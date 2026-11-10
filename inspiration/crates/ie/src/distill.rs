//! 炼化：把一张图变成「可复现的文字配方」。
//!
//! 与 `vision` 的区别：vision 产出的是入库索引（基元分数），
//! distill 产出的是给人/AI 直接复用的文本——构图、色彩（带 HEX）、文字、材质、
//! 可直接投喂图像模型的提示词，以及三条可迁移原则。

use anyhow::{Context, Result};
use base64::Engine;
use rusqlite::{Connection, params};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::metrics::{Metrics, compute};
use crate::vision::{CallOutcome, call, prepare_image};

pub struct Opts<'a> {
    pub db: &'a str,
    pub image: Option<&'a str>,
    pub work: Option<i64>,
    pub model: &'a str,
    pub base_url: &'a str,
    pub out_dir: &'a Path,
    pub thinking: u32,
}

pub fn run(opts: &Opts<'_>) -> Result<()> {
    let key = std::env::var("GEMINI_API_KEY").context("缺少 GEMINI_API_KEY")?;
    let (path, title, source) = resolve(opts)?;
    let m = compute(&path)?;
    let img_bytes = prepare_image(&path)?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&img_bytes);
    let prompt = build_prompt(&title, &m);
    let body = request_body(&prompt, &b64, opts.thinking);

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(180))
        .build()?;
    let json = match call(&client, opts.base_url, opts.model, &key, &body) {
        CallOutcome::Ok(v, _) => v,
        CallOutcome::RateLimited(i) => {
            anyhow::bail!("限流：{}（约 {}s 后重试）", i.body, i.retry_after)
        }
        CallOutcome::Failed(e) => anyhow::bail!("调用失败：{e}"),
    };

    let md = render(&title, &source, &m, &json, opts.model);
    std::fs::create_dir_all(opts.out_dir)?;
    let slug = slugify(&title);
    let out = opts.out_dir.join(format!("{slug}.md"));
    std::fs::write(&out, &md)?;
    save_spec(opts.db, &title, &source, &m, &json, &md)?;
    println!("{}", md);
    println!("\n已写入 {}", out.display());
    Ok(())
}

fn resolve(opts: &Opts<'_>) -> Result<(PathBuf, String, String)> {
    if let Some(p) = opts.image {
        let path = PathBuf::from(p);
        if !path.exists() {
            anyhow::bail!("找不到图片 {p}");
        }
        let title = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("untitled")
            .to_string();
        return Ok((path, title, format!("文件 {p}")));
    }
    let id = opts.work.context("需要 --image 或 --work")?;
    let conn = Connection::open(opts.db)?;
    let (local, title, collection, source_name): (String, String, String, String) = conn
        .query_row(
            "SELECT a.local_path, w.title, w.collection, s.title
         FROM asset a JOIN work w ON w.id = a.work_id JOIN source s ON s.id = w.source_id
         WHERE a.work_id = ?1 AND a.local_path IS NOT NULL LIMIT 1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
    let root = PathBuf::from("data/inspiration");
    Ok((
        root.join(local),
        title,
        format!("{source_name} · {collection} · work #{id}"),
    ))
}

fn build_prompt(title: &str, m: &Metrics) -> String {
    let palette = m
        .palette
        .iter()
        .map(|(hex, s)| format!("{hex} {:.0}%", s * 100.0))
        .collect::<Vec<_>>()
        .join("、");
    format!(
        r##"你是视觉配方师。任务不是评价这张图，而是把它**炼成可复现的配方**：
别人只读你的文字、不看原图，也能做出气质一致的画面。

已知像素实测（直接引用，不要重新猜）：
- 尺寸 {w}×{h}
- 主色板（按面积）：{palette}
- 留白率 {ws:.2}｜边缘密度 {ed:.2}｜明度对比 {ct:.1}:1｜对称度 {sy:.2}｜对齐度 {al:.2}

写作要求：
1. **区分实测与估计**：只有上面【已知像素实测】里的数字可以直接用；其他尺寸/半径/字号必须写成范围并标注"（估计）"，例如"圆角 16–24px（估计）"。绝不允许编造精确到个位的数值。
2. 禁止空泛词（高级、大气、精致、氛围感）单独出现，必须紧跟可执行的量或可检查的判断。
3. 颜色只用实测 HEX，占比写百分比。
4. 每一节至少给一条"可检查的判断"（比如"标题占画面宽度约 60%（估计）"）。
4. prompt_zh 与 prompt_en 必须自包含：不出现"原图/这张图/如上"，可直接投喂图像生成模型；
   要包含构图、主体、色彩关系、光线、材质、文字排版、情绪，各 120–200 字/词。
5. don'ts 写 3 条：复现时最容易做错的点（具体到可检查）。
6. principles 写 3 条可迁移原则，句式必须是"当…时，做…，因为…"，且删掉具体内容仍然成立。
7. one_line_recipe 是一句可以粘进任意提示词开头的风格串（≤50 字，含构图+色彩+质感）。
8. variants 写 3 条变体方向：保留 essence、改变一处结构，用于衍生。

只输出 JSON，字段如下（全部必填，字符串用中文，prompt_en 用英文）：
{{
 "essence": "一句话说清它为什么好看（≤40字）",
 "composition": {{"structure":"版式骨架","anchors":"视觉锚点及位置","hierarchy":"层级关系","weight":"重心位置与比例"}},
 "proportion": {{"ratios":"关键比例关系","spacing":"间距体系与倍数"}},
 "typography": {{"families":"字体风格判断","scale":"字号层级与倍数","weights":"字重与字距","treatment":"特殊处理"}},
 "graphics": {{"shapes":"图形语言","corners":"圆角/线宽","texture":"纹理与颗粒"}},
 "light": {{"direction":"光线方向","contrast":"明暗关系","material":"材质与反射"}},
 "rhythm": {{"density":"疏密分布","repetition":"重复与破格"}},
 "mood": {{"calm":0.0,"warm":0.0,"simple":0.0}},
 "prompt_zh": "自包含的中文复现提示词",
 "prompt_en": "self-contained English prompt",
 "tokens": {{"colors":["#RRGGBB"],"radius":"...","type_scale":"...","spacing":"..."}},
 "donts": ["...","...","..."],
 "principles": ["当…时，做…，因为…","...","..."],
 "one_line_recipe": "一句风格串",
 "variants": ["保留…，改成…","...","..."]
}}"##,
        w = m.width,
        h = m.height,
        palette = palette,
        ws = m.whitespace,
        ed = m.edge_density,
        ct = m.contrast,
        sy = m.symmetry,
        al = m.alignment,
    ) + &format!("\n参考标题：{title}")
}

fn request_body(prompt: &str, b64: &str, thinking: u32) -> serde_json::Value {
    serde_json::json!({
        "contents": [{"parts": [
            {"text": prompt},
            {"inline_data": {"mime_type": "image/jpeg", "data": b64}}
        ]}],
        "generationConfig": {
            "responseMimeType": "application/json",
            "temperature": 0.3,
            "maxOutputTokens": 3072,
            "thinkingConfig": {"thinkingBudget": thinking}
        }
    })
}

fn s(v: &serde_json::Value, path: &[&str]) -> String {
    let mut cur = v;
    for p in path {
        cur = match cur.get(p) {
            Some(x) => x,
            None => return String::new(),
        };
    }
    cur.as_str().unwrap_or("").to_string()
}

fn arr(v: &serde_json::Value, key: &str) -> Vec<String> {
    v.get(key)
        .and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

fn bar(v: f64) -> String {
    let n = (v.clamp(0.0, 1.0) * 10.0).round() as usize;
    format!("{}{}", "█".repeat(n), "·".repeat(10 - n))
}

fn render(title: &str, source: &str, m: &Metrics, j: &serde_json::Value, model: &str) -> String {
    let palette_rows = m
        .palette
        .iter()
        .enumerate()
        .map(|(i, (hex, share))| {
            format!(
                "| `{hex}` | {:.0}% | {} |",
                share * 100.0,
                if i == 0 { "底/主色" } else { "辅助" }
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let donts = arr(j, "donts")
        .iter()
        .map(|x| format!("- {x}"))
        .collect::<Vec<_>>()
        .join("\n");
    let principles = arr(j, "principles")
        .iter()
        .map(|x| format!("- {x}"))
        .collect::<Vec<_>>()
        .join("\n");
    let calm = j
        .pointer("/mood/calm")
        .and_then(|x| x.as_f64())
        .unwrap_or(0.5);
    let warm = j
        .pointer("/mood/warm")
        .and_then(|x| x.as_f64())
        .unwrap_or(0.5);
    let simple = j
        .pointer("/mood/simple")
        .and_then(|x| x.as_f64())
        .unwrap_or(0.5);

    format!(
        r##"# 炼化卡 · {title}

> {source} ｜ {w}×{h} ｜ {model}

## 一句话
{essence}

## 视觉配方

### 构图
- **骨架**：{structure}
- **锚点**：{anchors}
- **层级**：{hierarchy}
- **重心**：{weight}

### 比例与间距
- **比例**：{ratios}
- **间距**：{spacing}

### 色彩（像素实测）
| HEX | 占比 | 角色 |
| --- | --- | --- |
{palette_rows}

- 留白率 {ws:.2}｜边缘密度 {ed:.2}｜明度对比 {ct:.1}:1｜对称 {sy:.2}｜对齐 {al:.2}

### 文字
- **字体气质**：{families}
- **字号层级**：{scale}
- **字重字距**：{weights}
- **特殊处理**：{treatment}

### 图形与材质
- **图形**：{shapes}
- **圆角/线宽**：{corners}
- **纹理**：{texture}

### 光
- **方向**：{direction}
- **对比**：{contrast}
- **材质**：{material}

### 节奏
- **疏密**：{density}
- **重复与破格**：{repetition}

### 情绪坐标
```text
静 ← calm     {calm_bar} → 动
冷 ← warm     {warm_bar} → 暖
繁 ← simple   {simple_bar} → 简
```

## 一句话配方（可直接粘贴）
> {one_line}

## 复现提示词

### 中文（直接可用）
> {prompt_zh}

### English
> {prompt_en}

### 给代码生成器的 tokens
```json
{tokens}
```

## 不要做什么
{donts}

## 可迁移原则
{principles}

## 变体方向
{variants}
"##,
        title = title,
        source = source,
        model = model,
        w = m.width,
        h = m.height,
        essence = s(j, &["essence"]),
        structure = s(j, &["composition", "structure"]),
        anchors = s(j, &["composition", "anchors"]),
        hierarchy = s(j, &["composition", "hierarchy"]),
        weight = s(j, &["composition", "weight"]),
        ratios = s(j, &["proportion", "ratios"]),
        spacing = s(j, &["proportion", "spacing"]),
        palette_rows = palette_rows,
        ws = m.whitespace,
        ed = m.edge_density,
        ct = m.contrast,
        sy = m.symmetry,
        al = m.alignment,
        families = s(j, &["typography", "families"]),
        scale = s(j, &["typography", "scale"]),
        weights = s(j, &["typography", "weights"]),
        treatment = s(j, &["typography", "treatment"]),
        shapes = s(j, &["graphics", "shapes"]),
        corners = s(j, &["graphics", "corners"]),
        texture = s(j, &["graphics", "texture"]),
        direction = s(j, &["light", "direction"]),
        contrast = s(j, &["light", "contrast"]),
        material = s(j, &["light", "material"]),
        density = s(j, &["rhythm", "density"]),
        repetition = s(j, &["rhythm", "repetition"]),
        calm_bar = bar(calm),
        warm_bar = bar(warm),
        simple_bar = bar(simple),
        one_line = s(j, &["one_line_recipe"]),
        variants = arr(j, "variants")
            .iter()
            .map(|x| format!("- {x}"))
            .collect::<Vec<_>>()
            .join("\n"),
        prompt_zh = s(j, &["prompt_zh"]),
        prompt_en = s(j, &["prompt_en"]),
        tokens = serde_json::to_string_pretty(j.get("tokens").unwrap_or(&serde_json::Value::Null))
            .unwrap_or_default(),
        donts = donts,
        principles = principles,
    )
}

fn save_spec(
    db: &str,
    title: &str,
    source: &str,
    m: &Metrics,
    j: &serde_json::Value,
    md: &str,
) -> Result<()> {
    let conn = Connection::open(db)?;
    conn.busy_timeout(Duration::from_secs(10))?;
    let now = crate::now_iso();
    conn.execute(
        "INSERT INTO spec(domain_id, name, profile_json, body_md, tokens_json, created_at, updated_at)
         VALUES('ui', ?1, ?2, ?3, ?4, ?5, ?5)",
        params![
            format!("炼化 · {title}"),
            serde_json::json!({
                "source": source,
                "width": m.width, "height": m.height,
                "palette": m.palette,
                "whitespace": m.whitespace, "contrast": m.contrast,
                "recipe": j,
            })
            .to_string(),
            md,
            j.get("tokens").unwrap_or(&serde_json::Value::Null).to_string(),
            now
        ],
    )?;
    Ok(())
}

fn slugify(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if ch.is_whitespace() || ch == '-' || ch == '_' {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        format!("distill-{}", crate::now_iso().replace(':', ""))
    } else {
        trimmed.chars().take(60).collect()
    }
}
