import { useEffect, useMemo, useRef, useState } from 'react'
import api from '../api/client'
import type { AiActionRunRead, EpisodeRead, SceneRead, ShotRead, SplitSceneItem, SplitShotItem } from '../api/types'
import { useProjectSelection } from '../state/useProjectSelection'

type SplitScenePreview = SplitSceneItem & { _key: string }
type SplitShotPreview = SplitShotItem & {
  _key: string
  prompt?: string
  negative_prompt?: string
  shot_size?: string
  camera_angle?: string
  lighting_style?: string
}

type Selected =
  | { kind: 'episode'; episodeId: number }
  | { kind: 'scene'; episodeId: number; sceneId: number }
  | { kind: 'shot'; episodeId: number; sceneId: number; shotId: number }
  | { kind: 'none' }

function stripMarkdownCodeFences(input: string): string {
  const raw = String(input ?? '')
  const t = raw.trim()
  // 整段被 ```lang ... ``` 包裹
  const m = t.match(/^```[a-zA-Z0-9_-]*\s*\n([\s\S]*?)\n```$/)
  if (m && typeof m[1] === 'string') return m[1].trim()
  // 有些模型会输出 ```json{...}```（无换行）
  if (t.startsWith('```') && t.endsWith('```')) {
    const withoutStart = t.replace(/^```[a-zA-Z0-9_-]*\s*/i, '')
    const withoutEnd = withoutStart.replace(/\s*```$/i, '')
    return withoutEnd.trim()
  }
  return t
}

function sanitizeOutputForDisplay(input: string): string {
  let t = stripMarkdownCodeFences(input)
  // 如果是 JSON，展示时做格式化（更易读），失败则保持原样
  const s = t.trim()
  if ((s.startsWith('{') && s.endsWith('}')) || (s.startsWith('[') && s.endsWith(']'))) {
    try {
      return JSON.stringify(JSON.parse(s), null, 2)
    } catch {
      // keep original
    }
  }
  return t
}

function tryParseJsonObject(input: string): any | null {
  const t = stripMarkdownCodeFences(input).trim()
  if (!t) return null
  try {
    const parsed = JSON.parse(t)
    return parsed && typeof parsed === 'object' ? parsed : null
  } catch {
    return null
  }
}

function pickFirst(obj: any, keys: string[]): any {
  if (!obj || typeof obj !== 'object') return undefined
  const lowerMap: Record<string, any> = {}
  for (const k of Object.keys(obj)) lowerMap[String(k).toLowerCase()] = (obj as any)[k]
  for (const k of keys) {
    const v = lowerMap[k.toLowerCase()]
    if (v !== undefined && v !== null) return v
  }
  return undefined
}

function extractActs(outline: any): { key: string; title: string; data: any }[] {
  if (!outline || typeof outline !== 'object') return []
  const acts: { key: string; title: string; data: any }[] = []

  const collectFrom = (obj: any, prefix: string) => {
    if (!obj || typeof obj !== 'object') return
    for (const k of Object.keys(obj)) {
      const lk = k.toLowerCase().replace(/\s+/g, '')
      if (lk === 'act1' || lk === 'act_1' || lk === 'act-1')
        acts.push({ key: `${prefix}${k}`, title: '第一幕（act_1）', data: obj[k] })
      if (lk === 'act2' || lk === 'act_2' || lk === 'act-2')
        acts.push({ key: `${prefix}${k}`, title: '第二幕（act_2）', data: obj[k] })
      if (lk === 'act3' || lk === 'act_3' || lk === 'act-3')
        acts.push({ key: `${prefix}${k}`, title: '第三幕（act_3）', data: obj[k] })
    }
  }

  // 递归/BFS 搜索：很多输出会包一层 output_schema / result / data 等
  const visited = new Set<any>()
  const queue: Array<{ obj: any; path: string; depth: number }> = [{ obj: outline, path: '', depth: 0 }]
  const maxDepth = 6
  while (queue.length) {
    const cur = queue.shift()!
    const obj = cur.obj
    if (!obj || typeof obj !== 'object') continue
    if (visited.has(obj)) continue
    visited.add(obj)

    // 只要当前对象含有 act_1/2/3，直接收集
    collectFrom(obj, cur.path)

    if (cur.depth >= maxDepth) continue

    // 常见 wrapper 优先展开
    const wrappers = [
      pickFirst(obj, ['output_schema', 'outputSchema', 'schema', 'result', 'data', 'payload']),
      pickFirst(obj, ['optimized_beat_sheet', 'optimized_beatsheet', 'optimizedBeatSheet']),
      pickFirst(obj, ['beat_sheet', 'beatsheet', 'beatSheet']),
      pickFirst(obj, ['acts', 'act', 'three_act', 'threeact', 'three_act_structure', 'threeactstructure', 'act_structure', 'actstructure']),
    ].filter(Boolean)
    for (let i = 0; i < wrappers.length; i++) {
      queue.push({ obj: wrappers[i], path: cur.path + `w${cur.depth}.${i}.`, depth: cur.depth + 1 })
    }

    // 普通字段也适度展开（避免漏掉 output_schema.optimized_beat_sheet 这种深层组合）
    for (const k of Object.keys(obj)) {
      const v = (obj as any)[k]
      if (v && typeof v === 'object') {
        queue.push({ obj: v, path: cur.path + `${k}.`, depth: cur.depth + 1 })
      }
    }
  }

  // 去重 + 排序
  const seen = new Set<string>()
  const uniq = acts.filter((a) => {
    const sig = `${a.title}:${a.key}`
    if (seen.has(sig)) return false
    seen.add(sig)
    return true
  })
  const order = { '第一幕（act_1）': 1, '第二幕（act_2）': 2, '第三幕（act_3）': 3 } as any
  uniq.sort((a, b) => (order[a.title] || 99) - (order[b.title] || 99))
  return uniq
}

function looksLikeTruncatedJson(input: string): boolean {
  const t = stripMarkdownCodeFences(input).trim()
  if (!t) return false
  const startsJson = t.startsWith('{') || t.startsWith('[')
  if (!startsJson) return false
  try {
    JSON.parse(t)
    return false
  } catch {
    // parse fail 且末尾明显没闭合时，基本可判定为截断
    if (t.startsWith('{') && !t.endsWith('}')) return true
    if (t.startsWith('[') && !t.endsWith(']')) return true
    return true
  }
}

export function ScriptPage() {
  const { projects, projectId, setProjectId, refreshProjects } = useProjectSelection()

  const [episodes, setEpisodes] = useState<EpisodeRead[]>([])
  const [selected, setSelected] = useState<Selected>({ kind: 'none' })

  // 项目大纲状态
  const [showOutlinePanel, setShowOutlinePanel] = useState(false)
  const [outlineInput, setOutlineInput] = useState('')
  const [outlineJson, setOutlineJson] = useState('')
  const [outlineError, setOutlineError] = useState<string | null>(null)
  const [loadingOutline, setLoadingOutline] = useState(false)
  const [generatingOutline, setGeneratingOutline] = useState(false)
  const [optimizingOutline, setOptimizingOutline] = useState(false)
  const [numEpisodes, setNumEpisodes] = useState(12)
  const [optimizeInstructions, setOptimizeInstructions] = useState('')

  // Chat（对话驱动优化）
  type ChatRole = 'user' | 'assistant'
  type ChatMsg = { id: string; role: ChatRole; content: string; ts: number; debug?: any }
  const [chatMsgs, setChatMsgs] = useState<ChatMsg[]>([])
  const [chatInput, setChatInput] = useState('')
  const [chatBusy, setChatBusy] = useState(false)
  const [chatError, setChatError] = useState<string | null>(null)
  const [chatDebug, setChatDebug] = useState(false)
  const [lastChatDebug, setLastChatDebug] = useState<any>(null)

  const [episodeDescription, setEpisodeDescription] = useState('')
  const [sceneDescription, setSceneDescription] = useState('')
  const [episodeDirty, setEpisodeDirty] = useState(false)
  const [sceneDirty, setSceneDirty] = useState(false)
  const [workstationDirty, setWorkstationDirty] = useState(false)

  const prevProjectIdRef = useRef<number | null>(null)
  const episodeDirtyForIdRef = useRef<number | null>(null)
  const sceneDirtyForIdRef = useRef<number | null>(null)
  const workstationDirtyKeyRef = useRef<string | null>(null)

  function _lsGet(key: string): string | null {
    try {
      return window.localStorage.getItem(key)
    } catch {
      return null
    }
  }
  function _lsSet(key: string, value: string) {
    try {
      window.localStorage.setItem(key, value)
    } catch {
      // ignore
    }
  }
  function _lsDel(key: string) {
    try {
      window.localStorage.removeItem(key)
    } catch {
      // ignore
    }
  }
  function _lsKeys(): string[] {
    try {
      return Object.keys(window.localStorage || {})
    } catch {
      return []
    }
  }
  function _draftKeyEpisode(pid: number, episodeId: number) {
    return `aicomic.script.draft.episode.${pid}.${episodeId}`
  }
  function _draftKeyScene(pid: number, sceneId: number) {
    return `aicomic.script.draft.scene.${pid}.${sceneId}`
  }
  function _draftKeyWorkstation(pid: number, episodeId: number, tab: string) {
    return `aicomic.script.draft.workstation.${pid}.${episodeId}.${tab}`
  }
  function _selectedKey(pid: number) {
    return `aicomic.script.selected.${pid}`
  }
  function _chatKey(pid: number, episodeId: number) {
    return `aicomic.script.chat.${pid}.${episodeId}`
  }

  function clearProjectLocalCache(pid: number) {
    // 只清理本页相关的 key，避免误删其它模块
    const prefixes = [
      `aicomic.script.draft.episode.${pid}.`,
      `aicomic.script.draft.scene.${pid}.`,
      `aicomic.script.draft.workstation.${pid}.`,
      `aicomic.script.chat.${pid}.`,
      `aicomic.script.selected.${pid}`,
      // useProjectSelection 的项目选择缓存（删项目后避免仍指向已删除 id）
      'aicomic.projectId',
    ]
    for (const k of _lsKeys()) {
      if (prefixes.some((p) => (p.endsWith('.') ? k.startsWith(p) : k === p || k.startsWith(p)))) {
        _lsDel(k)
      }
    }
  }
  function _readJson<T = any>(key: string): T | null {
    const raw = _lsGet(key)
    if (!raw) return null
    try {
      return JSON.parse(raw) as T
    } catch {
      return null
    }
  }

  // 新建项目（避免使用 window.prompt，兼容 Tauri WebView）
  const [showCreateProject, setShowCreateProject] = useState(false)
  const [createProjectName, setCreateProjectName] = useState('')
  const [createProjectDesc, setCreateProjectDesc] = useState('')
  const [creatingProject, setCreatingProject] = useState(false)
  const [createProjectError, setCreateProjectError] = useState<string | null>(null)

  // 项目重命名/删除（兼容 Tauri WebView）
  const [showRenameProject, setShowRenameProject] = useState(false)
  const [renameProjectName, setRenameProjectName] = useState('')
  const [renameProjectDesc, setRenameProjectDesc] = useState('')
  const [renamingProject, setRenamingProject] = useState(false)
  const [renameProjectError, setRenameProjectError] = useState<string | null>(null)

  const [showDeleteProject, setShowDeleteProject] = useState(false)
  const [deletingProject, setDeletingProject] = useState(false)
  const [deleteProjectError, setDeleteProjectError] = useState<string | null>(null)

  // 新建集（避免使用 window.prompt，兼容 Tauri WebView）
  const [showCreateEpisode, setShowCreateEpisode] = useState(false)
  const [createEpisodeTitle, setCreateEpisodeTitle] = useState('')
  const [createEpisodeError, setCreateEpisodeError] = useState<string | null>(null)
  const [creatingEpisode, setCreatingEpisode] = useState(false)

  // 新建场（避免使用 window.prompt，兼容 Tauri WebView）
  const [showCreateScene, setShowCreateScene] = useState(false)
  const [createSceneEpisodeId, setCreateSceneEpisodeId] = useState<number | null>(null)
  const [createSceneTitle, setCreateSceneTitle] = useState('')
  const [createSceneError, setCreateSceneError] = useState<string | null>(null)
  const [creatingScene, setCreatingScene] = useState(false)

  const [splitScenesPreview, setSplitScenesPreview] = useState<SplitScenePreview[]>([])
  const [isSplitting, setIsSplitting] = useState(false)
  const [splitError, setSplitError] = useState<string | null>(null)
  const [overwriteOnImport, setOverwriteOnImport] = useState(false)
  const [isImporting, setIsImporting] = useState(false)

  const [storyboardPreview, setStoryboardPreview] = useState<SplitShotPreview[]>([])
  const [isStoryboardSplitting, setIsStoryboardSplitting] = useState(false)
  const [storyboardError, setStoryboardError] = useState<string | null>(null)
  const [overwriteShotsOnImport, setOverwriteShotsOnImport] = useState(false)
  const [isStoryboardImporting, setIsStoryboardImporting] = useState(false)

  const [shotDraft, setShotDraft] = useState<Partial<ShotRead> | null>(null)

  // AI 写作：大纲生成 / 剧本生成（结果预览后再应用）
  const [aiWritingBusy, setAiWritingBusy] = useState<'outline' | 'script' | null>(null)
  const [aiWritingError, setAiWritingError] = useState<string | null>(null)
  const [aiResult, setAiResult] = useState<{ title: string; text: string } | null>(null)
  const [outlinePreview, setOutlinePreview] = useState<{ raw: string; parsed: any | null } | null>(null)
  const [outlinePreviewMode, setOutlinePreviewMode] = useState<'preview' | 'raw'>('preview')

  // Workflow：生成与应用（run_id 驱动 apply-to-db）
  const [workflowBusy, setWorkflowBusy] = useState<'script' | 'storyboard' | 'apply_script' | 'apply_storyboard' | null>(null)
  const [workflowError, setWorkflowError] = useState<string | null>(null)
  const [lastWorkflowRunId, setLastWorkflowRunId] = useState<string | null>(null)
  const [lastWorkflowKind, setLastWorkflowKind] = useState<'script' | 'storyboard' | null>(null)
  const [storyboardPromptStyle, setStoryboardPromptStyle] = useState<'sd_tags' | 'mj_v6'>('sd_tags')
  const [storyboardAspectRatio, setStoryboardAspectRatio] = useState<string>('')

  // EP 按钮输出历史（DB）
  type EpActionKey = 'outline_generate' | 'generate_script' | 'script_optimize' | 'split_scenes'
  const EP_ACTIONS: { key: EpActionKey; title: string; canApply: boolean }[] = [
    { key: 'outline_generate', title: '大纲生成', canApply: true },
    { key: 'generate_script', title: '剧本生成', canApply: true },
    { key: 'script_optimize', title: '剧本优化', canApply: true },
    { key: 'split_scenes', title: '自动分场', canApply: true },
  ]
  const [epActionTab, setEpActionTab] = useState<EpActionKey>('outline_generate')
  const [workstationInput, setWorkstationInput] = useState('')

  const [epRuns, setEpRuns] = useState<Record<EpActionKey, AiActionRunRead[]>>({
    outline_generate: [],
    generate_script: [],
    script_optimize: [],
    split_scenes: [],
  })
  const [, setEpSelectedRunId] = useState<Record<EpActionKey, number | null>>({
    outline_generate: null,
    generate_script: null,
    script_optimize: null,
    split_scenes: null,
  })

  // 项目列表与 projectId 由 useProjectSelection 统一管理（含 localStorage 记忆）

  const selectedEpisode = useMemo(() => {
    if (selected.kind === 'episode') return episodes.find((e) => e.id === selected.episodeId) || null
    if (selected.kind === 'scene' || selected.kind === 'shot')
      return episodes.find((e) => e.id === selected.episodeId) || null
    return null
  }, [episodes, selected])

  const selectedScene = useMemo(() => {
    if (selected.kind !== 'scene' && selected.kind !== 'shot') return null
    const ep = episodes.find((e) => e.id === selected.episodeId)
    return ep?.scenes?.find((s) => s.id === selected.sceneId) || null
  }, [episodes, selected])

  const selectedShot = useMemo(() => {
    if (selected.kind !== 'shot') return null
    const ep = episodes.find((e) => e.id === selected.episodeId)
    const sc = ep?.scenes?.find((s) => s.id === selected.sceneId)
    return sc?.shots?.find((sh) => sh.id === selected.shotId) || null
  }, [episodes, selected])

  async function refreshScript(nextProjectId = projectId) {
    if (!nextProjectId) return
    const res = await api.getScript(nextProjectId)
    setEpisodes(res.data || [])
    // 注意：不要在 refresh 时清空编辑器状态，否则切换页面/回来会导致输入丢失。
    // 如需彻底重置（例如切换 project），在外层 effect 中处理。
  }

  const currentProject = useMemo(() => {
    if (!projectId) return null
    return projects.find((p) => p.id === projectId) || null
  }, [projects, projectId])

  useEffect(() => {
    const prev = prevProjectIdRef.current
    prevProjectIdRef.current = projectId ?? null

    // project 切换：重置 UI（这是用户预期的“切项目清空”）
    if (prev !== null && projectId && prev !== projectId) {
      setSelected({ kind: 'none' })
      setEpisodeDescription('')
      setSceneDescription('')
      setEpisodeDirty(false)
      setSceneDirty(false)
      setWorkstationDirty(false)
      setSplitScenesPreview([])
      setStoryboardPreview([])
      setShotDraft(null)
      setEpActionTab('outline_generate')
      setWorkstationInput('')
      setEpRuns({
        outline_generate: [],
        generate_script: [],
        script_optimize: [],
        split_scenes: [],
      })
      setEpSelectedRunId({
        outline_generate: null,
        generate_script: null,
        script_optimize: null,
        split_scenes: null,
      })
    }

    refreshScript().catch(() => {
      // ignore
    })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projectId])

  // 记住/恢复当前选中项（避免切换页面回来后 selection 丢失）
  useEffect(() => {
    if (!projectId) return
    _lsSet(_selectedKey(projectId), JSON.stringify(selected))
  }, [projectId, selected])

  // 恢复选中项（仅当当前为 none 且数据已加载）
  useEffect(() => {
    if (!projectId) return
    if (selected.kind !== 'none') return
    if (!episodes.length) return
    const saved = _readJson<Selected>(_selectedKey(projectId))
    if (!saved || typeof saved !== 'object' || !('kind' in saved)) return
    // 校验 saved 是否仍然存在
    const ok =
      saved.kind === 'episode'
        ? episodes.some((e) => e.id === (saved as any).episodeId)
        : saved.kind === 'scene' || saved.kind === 'shot'
          ? episodes.some((e) => e.id === (saved as any).episodeId)
          : false
    if (ok) setSelected(saved)
    else _lsDel(_selectedKey(projectId))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [episodes, projectId, selected.kind])

  // 同步选中对象的 description 到右侧编辑区（优先使用本地草稿；避免覆盖用户手动编辑）
  useEffect(() => {
    if (!projectId) return
    if (selected.kind === 'episode') {
      // 切换到不同 episode：重置 dirty（dirty 只应作用于当前选中对象）
      if (episodeDirtyForIdRef.current !== selected.episodeId) {
        episodeDirtyForIdRef.current = selected.episodeId
        setEpisodeDirty(false)
      }
      const ep = episodes.find((e) => e.id === selected.episodeId)
      const draft = _lsGet(_draftKeyEpisode(projectId, selected.episodeId))
      // 只有在未 dirty 时才会跟随 DB 刷新；dirty 时保持用户输入
      if (!episodeDirty) setEpisodeDescription(draft ?? ep?.description ?? '')
      setSceneDescription('')
      setSceneDirty(false)
      setSplitScenesPreview([])
      setSplitError(null)
      refreshEpRuns(selected.episodeId).catch(() => {})
      return
    }
    if (selected.kind === 'scene') {
      if (sceneDirtyForIdRef.current !== selected.sceneId) {
        sceneDirtyForIdRef.current = selected.sceneId
        setSceneDirty(false)
      }
      const ep = episodes.find((e) => e.id === selected.episodeId)
      const sc = ep?.scenes?.find((s) => s.id === selected.sceneId)
      const draft = _lsGet(_draftKeyScene(projectId, selected.sceneId))
      if (!sceneDirty) setSceneDescription(draft ?? sc?.description ?? '')
      setEpisodeDescription('')
      setEpisodeDirty(false)
      setStoryboardPreview([])
      setStoryboardError(null)
      return
    }
    if (selected.kind === 'shot') {
      const sh = selectedShot
      if (sh) setShotDraft(JSON.parse(JSON.stringify(sh)) as ShotRead)
      return
    }
    setShotDraft(null)
  }, [episodes, projectId, selected, selectedShot, episodeDirty, sceneDirty])

  // 自动填充 Input 的逻辑（不覆盖用户手动输入；并持久化到本地）
  useEffect(() => {
    if (!projectId) return
    if (selected.kind !== 'episode') return
    const epId = selected.episodeId

    // 如果该 tab 已有草稿，优先恢复草稿（用于“切换页面再回来”）
    const wsKey = _draftKeyWorkstation(projectId, epId, epActionTab)
    // 切换 episode/tab：重置 dirty（dirty 仅针对当前 wsKey）
    if (workstationDirtyKeyRef.current !== wsKey) {
      workstationDirtyKeyRef.current = null
      setWorkstationDirty(false)
    }
    const draft = _lsGet(wsKey)
    if (!workstationDirty && typeof draft === 'string') {
      setWorkstationInput(draft)
      return
    }

    // 切换 Tab 时，若 input 为空，尝试自动填充
    // 逻辑：
    // outline_generate -> 留空 (由用户填 logline)
    // generate_script -> 优先取 outline_generate 的最新 output，否则取 episodeDescription
    // script_optimize -> 优先取 generate_script 的最新 output，否则取 episodeDescription
    // split_scenes -> 优先取 script_optimize 的最新 output，否则取 episodeDescription

    let candidate = ''
    if (epActionTab === 'outline_generate') {
      // blank
    } else if (epActionTab === 'generate_script') {
      candidate = epRuns.outline_generate?.[0]?.output_text || episodeDescription
    } else if (epActionTab === 'script_optimize') {
      candidate = epRuns.generate_script?.[0]?.output_text || episodeDescription
    } else if (epActionTab === 'split_scenes') {
      candidate = epRuns.script_optimize?.[0]?.output_text || episodeDescription
    }
    
    // 仅当用户尚未手动编辑或当前 input 为空时才自动填充，避免覆盖用户输入
    if (!workstationDirty || !(workstationInput || '').trim()) {
      setWorkstationInput(candidate)
      _lsSet(wsKey, candidate)
    }
  }, [epActionTab, projectId, selected, epRuns, episodeDescription, workstationDirty, workstationInput])

  async function refreshEpRuns(episodeId: number) {
    if (!projectId) return
    const epId = episodeId
    try {
      const results = await Promise.all(
        EP_ACTIONS.map((a) => api.getAiRuns({ project_id: projectId, episode_id: epId, action_key: a.key, limit: 50 })),
      )
      const next: any = {}
      const nextSel: any = {}
      EP_ACTIONS.forEach((a, idx) => {
        const list = results[idx].data || []
        next[a.key] = list
        nextSel[a.key] = list.length ? list[0].id : null
      })
      setEpRuns(next)
      setEpSelectedRunId(nextSel)
    } catch {
      // ignore（不阻断主流程）
    }
  }

  async function appendEpRun(action_key: EpActionKey, input_text: string | null, output_text: string, meta_data?: Record<string, unknown>) {
    if (!projectId) return
    if (selected.kind !== 'episode') return
    const episodeId = selected.episodeId
    try {
      const res = await api.createAiRun({
        project_id: projectId,
        target_type: 'episode',
        target_id: episodeId,
        action_key,
        input_text,
        output_text,
        meta_data: meta_data || null,
      })
      const created = res.data
      setEpRuns((prev) => ({ ...prev, [action_key]: [created, ...(prev[action_key] || [])] }))
      setEpSelectedRunId((prev) => ({ ...prev, [action_key]: created.id }))
      setEpActionTab(action_key)
    } catch {
      // ignore
    }
  }

  function applyEpRunToEditor(run: AiActionRunRead | null) {
    if (!run) return
    if (epActionTab === 'split_scenes') {
      // split_scenes: 尝试把 output_text 解析为分场列表，写入预览区
      try {
        const parsed = JSON.parse(run.output_text || 'null')
        if (Array.isArray(parsed)) {
          setSplitScenesPreview(
            parsed.map((x: any, idx: number) => ({
              _key: `${run.id}-${idx}`,
              title: String(x?.title || `场${idx + 1}`),
              description: String(x?.description || ''),
            })),
          )
          return
        }
      } catch {
        // fallthrough
      }
    }
    setEpisodeDirty(true)
    setEpisodeDescription(run.output_text || '')
    if (projectId && selected.kind === 'episode') {
      _lsSet(_draftKeyEpisode(projectId, selected.episodeId), run.output_text || '')
    }
  }

  async function deleteEpRun(runId: number, actionKey: EpActionKey) {
    if (!window.confirm('确定删除此条历史记录？')) return
    try {
      await api.deleteAiRun(runId)
      setEpRuns((prev) => ({
        ...prev,
        [actionKey]: (prev[actionKey] || []).filter((r) => r.id !== runId),
      }))
    } catch (e: any) {
      console.error('删除失败', e)
      alert('删除失败：' + (e?.message || '未知错误'))
    }
  }

  // Chat：加载/保存历史（本地）
  useEffect(() => {
    if (!projectId) return
    if (selected.kind !== 'episode') return
    const key = _chatKey(projectId, selected.episodeId)
    const raw = _lsGet(key)
    if (!raw) {
      setChatMsgs([])
      return
    }
    try {
      const parsed = JSON.parse(raw)
      if (Array.isArray(parsed)) setChatMsgs(parsed as any)
    } catch {
      setChatMsgs([])
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projectId, selected.kind === 'episode' ? selected.episodeId : null])

  useEffect(() => {
    if (!projectId) return
    if (selected.kind !== 'episode') return
    _lsSet(_chatKey(projectId, selected.episodeId), JSON.stringify(chatMsgs.slice(-50)))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [chatMsgs])

  async function sendChat() {
    if (!projectId) return
    if (selected.kind !== 'episode') return
    const msg = (chatInput || '').trim()
    if (!msg) return

    const userMsg: ChatMsg = { id: `${Date.now()}-u`, role: 'user', content: msg, ts: Date.now() }
    setChatMsgs((prev) => [...prev, userMsg])
    setChatInput('')
    setChatError(null)
    setChatBusy(true)
    try {
      const res = await api.aiChatAct({
        project_id: projectId,
        episode_id: selected.episodeId,
        current_action_key: epActionTab,
        message: msg,
        debug: chatDebug,
        ui_context: {
          master_script: episodeDescription,
          current_input: workstationInput,
          action_key: epActionTab,
        },
      })
      const data = res.data as any
      const assistantText = String(data?.assistant_message || '完成')
      const assistantMsg: ChatMsg = { id: `${Date.now()}-a`, role: 'assistant', content: assistantText, ts: Date.now(), debug: chatDebug ? data : undefined }
      setChatMsgs((prev) => [...prev, assistantMsg])
      if (chatDebug) setLastChatDebug(data)

      // 只写最终版本记录：后端已写入 AiActionRun，这里刷新当前 episode 的 runs
      await refreshEpRuns(selected.episodeId)
    } catch (e: any) {
      setChatError(e?.response?.data?.detail || e?.message || '执行失败')
    } finally {
      setChatBusy(false)
    }
  }

  // 项目大纲操作
  async function loadProjectOutline() {
    if (!projectId) return
    setLoadingOutline(true)
    setOutlineError(null)
    try {
      const res = await api.getProjectOutline(projectId, 'v1')
      const data = res.data as any
      if (data?.exists && data.data) {
        setOutlineJson(JSON.stringify(data.data, null, 2))
      } else {
        setOutlineJson('')
      }
    } catch (e: any) {
      setOutlineError(e?.response?.data?.detail || e?.message || '加载失败')
    } finally {
      setLoadingOutline(false)
    }
  }

  async function generateProjectOutline() {
    if (!projectId || !outlineInput.trim()) {
      setOutlineError('请输入故事灵感/概要')
      return
    }
    setGeneratingOutline(true)
    setOutlineError(null)
    try {
      const res = await api.aiProjectOutlineGenerate({
        project_id: projectId,
        input_text: outlineInput,
        num_episodes: numEpisodes,
      })
      if (res.data?.project_outline) {
        setOutlineJson(JSON.stringify(res.data.project_outline, null, 2))
      }
    } catch (e: any) {
      setOutlineError(e?.response?.data?.detail || e?.message || '生成失败')
    } finally {
      setGeneratingOutline(false)
    }
  }

  async function optimizeProjectOutline() {
    if (!projectId || !outlineJson.trim()) {
      setOutlineError('请先生成或加载大纲')
      return
    }
    setOptimizingOutline(true)
    setOutlineError(null)
    try {
      const res = await api.aiProjectOutlineOptimize({
        project_id: projectId,
        current_outline: outlineJson,
        optimization_instructions: optimizeInstructions,
      })
      if (res.data?.project_outline) {
        setOutlineJson(JSON.stringify(res.data.project_outline, null, 2))
        if (res.data.changes_summary) {
          alert(`优化完成：${res.data.changes_summary}`)
        }
      }
    } catch (e: any) {
      setOutlineError(e?.response?.data?.detail || e?.message || '优化失败')
    } finally {
      setOptimizingOutline(false)
    }
  }

  async function saveProjectOutline() {
    if (!projectId) return
    let parsed: Record<string, unknown>
    try {
      parsed = JSON.parse(outlineJson)
    } catch (e) {
      setOutlineError('JSON 格式无效')
      return
    }
    try {
      await api.putProjectOutline(projectId, { data: parsed, version: 'v1' })
      setOutlineError(null)
      alert('大纲已保存')
    } catch (e: any) {
      setOutlineError(e?.response?.data?.detail || e?.message || '保存失败')
    }
  }

  const nextEpisodeOrder = useMemo(() => {
    return episodes.length > 0 ? Math.max(...episodes.map((e) => e.order || 0)) + 1 : 1
  }, [episodes])

  async function createProject() {
    const name = (createProjectName || '').trim()
    if (!name) return
    setCreatingProject(true)
    setCreateProjectError(null)
    try {
      const res = await api.createProject({ name, description: (createProjectDesc || '').trim() || null })
      const newId = res.data?.id
      await refreshProjects()
      if (typeof newId === 'number') setProjectId(newId)
      setShowCreateProject(false)
      setCreateProjectName('')
      setCreateProjectDesc('')
    } catch (e: any) {
      setCreateProjectError(e?.response?.data?.detail || e?.message || '新建项目失败')
    } finally {
      setCreatingProject(false)
    }
  }

  async function updateProjectMeta() {
    if (!projectId) return
    const name = (renameProjectName || '').trim()
    if (!name) return
    setRenamingProject(true)
    setRenameProjectError(null)
    try {
      await api.updateProject(projectId, { name, description: (renameProjectDesc || '').trim() || null })
      await refreshProjects()
      setShowRenameProject(false)
    } catch (e: any) {
      setRenameProjectError(e?.response?.data?.detail || e?.message || '重命名失败')
    } finally {
      setRenamingProject(false)
    }
  }

  async function deleteCurrentProject() {
    if (!projectId) return
    const deletingPid = projectId
    setDeletingProject(true)
    setDeleteProjectError(null)
    try {
      await api.deleteProject(projectId)
      // 删除项目后必须清理本机草稿缓存；否则当 SQLite 复用旧 project_id 时会回显旧数据
      clearProjectLocalCache(deletingPid)
      // 先刷新项目列表（会自动选择第一个并同步 localStorage）
      await refreshProjects()
      setShowDeleteProject(false)
      // 清一下本页数据（后续 useEffect 会根据新的 projectId 自动加载）
      setEpisodes([])
      setSelected({ kind: 'none' })
      setEpisodeDescription('')
      setSceneDescription('')
      setSplitScenesPreview([])
      setStoryboardPreview([])
      setShotDraft(null)
      // 给用户一个明确反馈（避免误以为没删除成功）
      try {
        alert('项目已删除，并已清理本机草稿缓存')
      } catch {
        // ignore
      }
    } catch (e: any) {
      setDeleteProjectError(e?.response?.data?.detail || e?.message || '删除失败')
    } finally {
      setDeletingProject(false)
    }
  }

  async function createEpisode() {
    if (!projectId) return
    const title = (createEpisodeTitle || '').trim()
    if (!title) return
    setCreatingEpisode(true)
    setCreateEpisodeError(null)
    try {
      const res = await api.createEpisode(projectId, { title, order: nextEpisodeOrder })
      const newId = (res.data as any)?.id
      await refreshScript(projectId)
      if (typeof newId === 'number') setSelected({ kind: 'episode', episodeId: newId })
      setShowCreateEpisode(false)
      setCreateEpisodeTitle('')
    } catch (e: any) {
      setCreateEpisodeError(e?.response?.data?.detail || e?.message || '新建失败')
    } finally {
      setCreatingEpisode(false)
    }
  }

  function openCreateSceneModal(episodeId: number) {
    const ep = episodes.find((e) => e.id === episodeId)
    const nextSeq =
      (ep?.scenes?.length || 0) > 0 ? Math.max(...(ep?.scenes || []).map((s) => s.sequence_number || 0)) + 1 : 1
    setCreateSceneEpisodeId(episodeId)
    setCreateSceneTitle(`场景 ${nextSeq}`)
    setCreateSceneError(null)
    setShowCreateScene(true)
  }

  async function createScene() {
    if (!projectId) return
    if (!createSceneEpisodeId) return
    const title = (createSceneTitle || '').trim()
    if (!title) return
    setCreatingScene(true)
    setCreateSceneError(null)
    try {
      const ep = episodes.find((e) => e.id === createSceneEpisodeId)
      const nextSeq =
        (ep?.scenes?.length || 0) > 0 ? Math.max(...(ep?.scenes || []).map((s) => s.sequence_number || 0)) + 1 : 1
      await api.createScene(createSceneEpisodeId, { title, sequence_number: nextSeq })
      await refreshScript(projectId)
      // 保持在该 episode，方便连续加场
      setSelected({ kind: 'episode', episodeId: createSceneEpisodeId })
      setShowCreateScene(false)
    } catch (e: any) {
      setCreateSceneError(e?.response?.data?.detail || e?.message || '新建场失败')
    } finally {
      setCreatingScene(false)
    }
  }

  async function addShot(sceneId: number) {
    const sc = selectedScene
    const nextSeq =
      (sc?.shots?.length || 0) > 0 ? Math.max(...(sc?.shots || []).map((s) => s.sequence_number || 0)) + 1 : 1
    await api.createShot(sceneId, { sequence_number: nextSeq, title: `镜头 ${nextSeq}`, action_text: '' })
    await refreshScript(projectId)
  }

  async function handleDeleteEpisode(episodeId: number) {
    if (!projectId) return
    await api.deleteEpisode(episodeId)
    // 删除后刷新并回到 none（由恢复逻辑决定是否自动选中）
    await refreshScript(projectId)
    setSelected({ kind: 'none' })
    setEpisodeDescription('')
    setSceneDescription('')
    setEpisodeDirty(false)
    setSceneDirty(false)
  }

  async function handleDeleteScene(sceneId: number) {
    if (!projectId) return
    await api.deleteScene(sceneId)
    await refreshScript(projectId)
    // 回到 episode 级
    if (selected.kind === 'scene' || selected.kind === 'shot') {
      setSelected({ kind: 'episode', episodeId: selected.episodeId })
    } else {
      setSelected({ kind: 'none' })
    }
    setSceneDescription('')
    setSceneDirty(false)
  }

  async function handleDeleteShot(shotId: number) {
    if (!projectId) return
    await api.deleteShot(shotId)
    await refreshScript(projectId)
    // 回到 scene 级
    if (selected.kind === 'shot') {
      setSelected({ kind: 'scene', episodeId: selected.episodeId, sceneId: selected.sceneId })
    }
  }

  async function saveEpisodeScript() {
    if (selected.kind !== 'episode') return
    await api.updateEpisode(selected.episodeId, { description: episodeDescription })
    await refreshScript(projectId)
    setSelected({ kind: 'episode', episodeId: selected.episodeId })
    // 保存成功：认为已落库，不再处于“未保存手动编辑”状态
    setEpisodeDirty(false)
    if (projectId) _lsDel(_draftKeyEpisode(projectId, selected.episodeId))
    episodeDirtyForIdRef.current = selected.episodeId
  }

  async function saveSceneScript() {
    if (selected.kind !== 'scene') return
    await api.updateScene(selected.sceneId, { description: sceneDescription })
    await refreshScript(projectId)
    setSelected({ kind: 'scene', episodeId: selected.episodeId, sceneId: selected.sceneId })
    setSceneDirty(false)
    if (projectId) _lsDel(_draftKeyScene(projectId, selected.sceneId))
    sceneDirtyForIdRef.current = selected.sceneId
  }

  async function saveShot() {
    if (selected.kind !== 'shot' || !shotDraft) return
    await api.updateShot(selected.shotId, {
      title: shotDraft.title,
      action_text: shotDraft.action_text,
      dialogue: shotDraft.dialogue,
      prompt: shotDraft.prompt,
      status: shotDraft.status,
    })
    await refreshScript(projectId)
    setSelected({ kind: 'shot', episodeId: selected.episodeId, sceneId: selected.sceneId, shotId: selected.shotId })
  }

  async function handleOutlineGenerate() {
    if (selected.kind !== 'episode') return
    const text = workstationInput.trim()
    if (!text) return
    setAiWritingBusy('outline')
    setAiWritingError(null)
    try {
      const res = await api.aiOutlineGenerate({ text })
      await appendEpRun('outline_generate', text, res.data?.text || '')
    } catch (e: any) {
      setAiWritingError(e?.response?.data?.detail || e?.message || '大纲生成失败')
    } finally {
      setAiWritingBusy(null)
    }
  }

  async function handleGenerateScript() {
    if (selected.kind !== 'episode') return
    const text = workstationInput.trim()
    if (!text) return
    setAiWritingBusy('script')
    setAiWritingError(null)
    try {
      const res = await api.aiGenerateScript({ text })
      await appendEpRun('generate_script', text, res.data?.text || '')
    } catch (e: any) {
      setAiWritingError(e?.response?.data?.detail || e?.message || '剧本生成失败')
    } finally {
      setAiWritingBusy(null)
    }
  }

  async function handleScriptOptimize() {
    if (selected.kind !== 'episode') return
    const text = workstationInput.trim()
    if (!text) return
    setAiWritingBusy('script')
    setAiWritingError(null)
    try {
      const res = await api.aiScriptOptimize({ text })
      await appendEpRun('script_optimize', text, res.data?.text || '')
    } catch (e: any) {
      setAiWritingError(e?.response?.data?.detail || e?.message || '剧本优化失败')
    } finally {
      setAiWritingBusy(null)
    }
  }

  async function handleAutoSplit() {
    if (selected.kind !== 'episode') return
    const text = workstationInput.trim()
    if (!text) return
    setIsSplitting(true)
    setSplitError(null)
    try {
      const res = await api.aiSplitScenes({ text })
      const list = res.data || []
      setSplitScenesPreview(
        list.map((it, idx) => ({
          _key: `${Date.now()}_${idx}`,
          title: it.title,
          description: it.description,
        })),
      )
      await appendEpRun(
        'split_scenes',
        text,
        JSON.stringify(list.map((x) => ({ title: x.title, description: x.description })), null, 2),
        { scenes: list.length },
      )
    } catch (e: any) {
      setSplitError(e?.response?.data?.detail || e?.message || '自动分场失败')
    } finally {
      setIsSplitting(false)
    }
  }

  async function handleImportScenes() {
    if (selected.kind !== 'episode') return
    const ep = selectedEpisode
    if (!ep) return
    if (splitScenesPreview.length === 0) return
    setIsImporting(true)
    try {
      if (overwriteOnImport && ep.scenes?.length) {
        for (const sc of ep.scenes) {
          await api.deleteScene(sc.id)
        }
      }
      // 逐个创建场（create_scene 当前仅写 title/sequence_number，所以再 patch 写 description）
      for (let i = 0; i < splitScenesPreview.length; i++) {
        const sc = splitScenesPreview[i]
        const createRes = await api.createScene(ep.id, { title: sc.title, sequence_number: i + 1 })
        const created = createRes.data as SceneRead
        await api.updateScene(created.id, { description: sc.description })
      }
      await refreshScript(projectId)
      setSelected({ kind: 'episode', episodeId: ep.id })
    } finally {
      setIsImporting(false)
    }
  }

  async function handleAutoStoryboard() {
    if (selected.kind !== 'scene') return
    const text = (sceneDescription || '').trim()
    if (!text) return
    setIsStoryboardSplitting(true)
    setStoryboardError(null)
    try {
      const res = await api.aiSplitShots({ text })
      const list = res.data || []
      setStoryboardPreview(
        list.map((it, idx) => ({
          _key: `${Date.now()}_${idx}`,
          title: it.title ?? null,
          action_text: it.action_text,
        })),
      )
    } catch (e: any) {
      setStoryboardError(e?.response?.data?.detail || e?.message || '自动分镜失败')
    } finally {
      setIsStoryboardSplitting(false)
    }
  }

  async function handleImportShots() {
    if (selected.kind !== 'scene') return
    const sc = selectedScene
    if (!sc) return
    if (storyboardPreview.length === 0) return
    setIsStoryboardImporting(true)
    try {
      if (overwriteShotsOnImport && sc.shots?.length) {
        for (const sh of sc.shots) {
          await api.deleteShot(sh.id)
        }
      }
      for (let i = 0; i < storyboardPreview.length; i++) {
        const sh = storyboardPreview[i]
        await api.createShot(sc.id, {
          sequence_number: i + 1,
          title: sh.title || `镜头 ${i + 1}`,
          action_text: sh.action_text,
          status: 'draft',
        })
      }
      await refreshScript(projectId)
      setSelected({ kind: 'scene', episodeId: selected.episodeId, sceneId: sc.id })
    } finally {
      setIsStoryboardImporting(false)
    }
  }

  // Workflow: Script (Episode 级别)
  async function handleWorkflowScript() {
    if (selected.kind !== 'episode' || !projectId) return
    const text = (episodeDescription || workstationInput || '').trim()
    if (!text) {
      setWorkflowError('请输入输入文本（大纲/章节/需求）')
      return
    }
    setWorkflowBusy('script')
    setWorkflowError(null)
    try {
      const res = await api.aiWorkflowScript({
        project_id: projectId,
        input_text: text,
        options: {
          qc_loops: 1,
          max_scenes: 50,
          derived_split_scenes: false,
        },
      })
      const data = res.data
      if (data) {
        setLastWorkflowRunId(data.run_id)
        setLastWorkflowKind('script')
        // 显示结果预览
        setAiResult({
          title: 'Workflow Script 生成结果',
          text: `Run ID: ${data.run_id}\n\nSeries Bible:\n${JSON.stringify(data.series_bible, null, 2)}\n\nBeat Sheet:\n${JSON.stringify(data.beat_sheet, null, 2)}\n\nScript Fountain:\n${data.script_fountain}\n\nQC Report:\n${JSON.stringify(data.qc_report, null, 2)}`,
        })
        // 可选：自动填充到 episodeDescription
        if (data.script_fountain) {
          setEpisodeDirty(true)
          setEpisodeDescription(data.script_fountain)
          _lsSet(_draftKeyEpisode(projectId, selected.episodeId), data.script_fountain)
          episodeDirtyForIdRef.current = selected.episodeId
        }
      }
    } catch (e: any) {
      setWorkflowError(e?.response?.data?.detail || e?.message || 'Workflow Script 失败')
    } finally {
      setWorkflowBusy(null)
    }
  }

  async function handleApplyWorkflowScript() {
    if (selected.kind !== 'episode' || !projectId || !lastWorkflowRunId || lastWorkflowKind !== 'script') return
    const ep = selectedEpisode
    if (!ep) return
    setWorkflowBusy('apply_script')
    setWorkflowError(null)
    try {
      await api.aiApplyWorkflowScript({
        project_id: projectId,
        episode_id: ep.id,
        run_id: lastWorkflowRunId,
        overwrite_scenes: false,
      })
      await refreshScript(projectId)
      setSelected({ kind: 'episode', episodeId: ep.id })
      setWorkflowError(null)
      setLastWorkflowRunId(null)
      setLastWorkflowKind(null)
    } catch (e: any) {
      setWorkflowError(e?.response?.data?.detail || e?.message || '应用 Workflow Script 失败')
    } finally {
      setWorkflowBusy(null)
    }
  }

  // Workflow: Storyboard (Scene 级别)
  async function handleWorkflowStoryboard() {
    if (selected.kind !== 'scene' || !projectId) return
    const text = (sceneDescription || '').trim()
    if (!text) {
      setWorkflowError('请输入场景文本')
      return
    }
    setWorkflowBusy('storyboard')
    setWorkflowError(null)
    try {
      const res = await api.aiWorkflowStoryboard({
        project_id: projectId,
        scene_text: text,
        options: {
          max_shots: 80,
          asset_item_ids: [],
          prompt_style: storyboardPromptStyle,
          aspect_ratio: storyboardAspectRatio || undefined,
        },
      })
      const data = res.data
      if (data && Array.isArray(data.shots)) {
        setLastWorkflowRunId(data.run_id)
        setLastWorkflowKind('storyboard')
        // 转换为预览格式
        setStoryboardPreview(
          data.shots.map((sh: any, idx: number) => ({
            _key: `${data.run_id}_${idx}`,
            title: sh.title || null,
            action_text: sh.action_text || '',
            prompt: sh.prompt || '',
            negative_prompt: sh.negative_prompt || '',
            shot_size: sh.shot_size || '',
            camera_angle: sh.camera_angle || '',
            lighting_style: sh.lighting_style || '',
          })),
        )
      }
    } catch (e: any) {
      setWorkflowError(e?.response?.data?.detail || e?.message || 'Workflow Storyboard 失败')
    } finally {
      setWorkflowBusy(null)
    }
  }

  async function handleApplyWorkflowStoryboard() {
    if (selected.kind !== 'scene' || !projectId || !lastWorkflowRunId || lastWorkflowKind !== 'storyboard') return
    const sc = selectedScene
    if (!sc) return
    setWorkflowBusy('apply_storyboard')
    setWorkflowError(null)
    try {
      await api.aiApplyWorkflowStoryboard({
        project_id: projectId,
        scene_id: sc.id,
        run_id: lastWorkflowRunId,
        overwrite_shots: true,
      })
      await refreshScript(projectId)
      setSelected({ kind: 'scene', episodeId: selected.episodeId, sceneId: sc.id })
      setWorkflowError(null)
      setLastWorkflowRunId(null)
      setLastWorkflowKind(null)
      setStoryboardPreview([])
    } catch (e: any) {
      setWorkflowError(e?.response?.data?.detail || e?.message || '应用 Workflow Storyboard 失败')
    } finally {
      setWorkflowBusy(null)
    }
  }

  return (
    <div style={styles.page}>
      <div style={styles.topbar}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
          <div style={{ fontWeight: 700 }}>剧本</div>
          <button
            style={{
              ...styles.btn,
              background: showOutlinePanel ? 'rgba(99,102,241,0.35)' : 'rgba(255,255,255,0.04)',
              border: showOutlinePanel ? '1px solid rgba(99,102,241,0.6)' : '1px solid rgba(255,255,255,0.14)',
            }}
            onClick={() => {
              setShowOutlinePanel(!showOutlinePanel)
              if (!showOutlinePanel && projectId) {
                loadProjectOutline().catch(() => {})
              }
            }}
          >
            📋 项目大纲
          </button>
        </div>
        <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
          <select
            value={projectId ?? ''}
            onChange={(e) => setProjectId(e.target.value ? Number(e.target.value) : null)}
            style={styles.select}
          >
            <option value="" disabled>
              选择项目…
            </option>
            {projects.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </select>
          <button
            onClick={() => {
              setCreateProjectError(null)
              setCreateProjectName('')
              setCreateProjectDesc('')
              setShowCreateProject(true)
            }}
            style={styles.btn}
          >
            + 新建项目
          </button>
          <button
            onClick={() => {
              if (!projectId) return
              setRenameProjectError(null)
              setRenameProjectName(currentProject?.name || '')
              setRenameProjectDesc(currentProject?.description || '')
              setShowRenameProject(true)
            }}
            style={styles.btn}
            disabled={!projectId}
          >
            重命名
          </button>
          <button
            onClick={() => {
              if (!projectId) return
              setDeleteProjectError(null)
              setShowDeleteProject(true)
            }}
            style={styles.btnDanger}
            disabled={!projectId}
            title="删除项目（含项目文件夹与关联数据）"
          >
            删除项目
          </button>
          <button onClick={() => refreshScript(projectId).catch(() => {})} style={styles.btn}>
            刷新
          </button>
          <button
            onClick={() => {
              if (!projectId) return
              setCreateEpisodeError(null)
              setCreateEpisodeTitle(`第${nextEpisodeOrder}集`)
              setShowCreateEpisode(true)
            }}
            style={styles.btnPrimary}
            disabled={!projectId}
          >
            + 新建集
          </button>
        </div>
      </div>

      {/* 项目大纲面板 */}
      {showOutlinePanel && (
        <div style={{
          padding: 14,
          borderRadius: 12,
          border: '1px solid rgba(255,255,255,0.08)',
          background: 'rgba(255,255,255,0.04)',
          marginBottom: 12,
        }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 12 }}>
            <div>
              <span style={{ fontWeight: 600, fontSize: 14 }}>📋 项目大纲</span>
              <span style={{ opacity: 0.6, fontSize: 11, marginLeft: 8 }}>整体故事概要 + 分集大纲</span>
            </div>
            <div style={{ display: 'flex', gap: 8 }}>
              <button style={styles.btn} onClick={() => loadProjectOutline().catch(() => {})} disabled={loadingOutline || !projectId}>
                {loadingOutline ? '加载中…' : '重新加载'}
              </button>
              <button style={styles.btnPrimary} onClick={() => saveProjectOutline().catch(() => {})} disabled={!projectId || !outlineJson}>
                保存
              </button>
              <button style={styles.btn} onClick={() => setShowOutlinePanel(false)}>
                收起
              </button>
            </div>
          </div>
          {outlineError ? <div style={{ color: '#f87171', fontSize: 12, marginBottom: 8 }}>{outlineError}</div> : null}
          
          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
            {/* 左侧：输入和生成 */}
            <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
              <div style={{ fontSize: 11, opacity: 0.6 }}>故事灵感/概要</div>
              <textarea
                value={outlineInput}
                onChange={(e) => setOutlineInput(e.target.value)}
                style={{ ...styles.textarea, height: 80 }}
                placeholder="输入故事灵感...（如：一个在山匪寨中长大的少女，发现自己是侯府遗孤...）"
                disabled={!projectId}
              />
              <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
                <span style={{ fontSize: 11, opacity: 0.6 }}>集数：</span>
                <input
                  type="number"
                  value={numEpisodes}
                  onChange={(e) => setNumEpisodes(Math.max(1, parseInt(e.target.value) || 12))}
                  style={{ ...styles.select, width: 50, textAlign: 'center', padding: '4px 6px' }}
                  min={1}
                  max={100}
                />
                <button
                  style={{ ...styles.btnPrimary, flex: 1 }}
                  onClick={() => generateProjectOutline().catch(() => {})}
                  disabled={generatingOutline || !projectId || !outlineInput.trim()}
                >
                  {generatingOutline ? '生成中…' : '🤖 AI 生成大纲'}
                </button>
              </div>
              <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
                <input
                  type="text"
                  value={optimizeInstructions}
                  onChange={(e) => setOptimizeInstructions(e.target.value)}
                  style={{ ...styles.select, flex: 1, padding: '6px 8px' }}
                  placeholder="优化指令（可选）"
                  disabled={!projectId}
                />
                <button
                  style={styles.btn}
                  onClick={() => optimizeProjectOutline().catch(() => {})}
                  disabled={optimizingOutline || !projectId || !outlineJson.trim()}
                >
                  {optimizingOutline ? '优化中…' : '✨ 优化'}
                </button>
              </div>
            </div>
            
            {/* 右侧：大纲结果 */}
            <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
              <div style={{ fontSize: 11, opacity: 0.6 }}>大纲结果 (JSON)</div>
              <textarea
                value={outlineJson}
                onChange={(e) => {
                  setOutlineJson(e.target.value)
                  setOutlineError(null)
                }}
                style={{ ...styles.textarea, height: 150, fontFamily: 'monospace', fontSize: 11 }}
                placeholder="点击「AI 生成大纲」或手动编辑..."
                disabled={!projectId}
              />
            </div>
          </div>
        </div>
      )}

      <div style={styles.body}>
        {/* 左：Episodes + Scenes 合并侧边栏 */}
        <div style={styles.sidebar}>
          {/* Episodes */}
          <div style={styles.sidebarSection}>
            <div style={styles.colHeader}>
              <div style={styles.colTitle}>Episodes</div>
            </div>
            <div style={styles.colScroll} className="aicomic-scroll">
              {episodes.map((ep) => (
                <div
                  key={ep.id}
                  style={{
                    ...styles.card,
                    ...(selected.kind !== 'none' && selectedEpisode?.id === ep.id ? styles.cardActive : null),
                  }}
                  onClick={() => setSelected({ kind: 'episode', episodeId: ep.id })}
                >
                  <div style={{ display: 'flex', justifyContent: 'space-between', gap: 8 }}>
                    <div style={{ minWidth: 0 }}>
                      <div style={styles.cardTitle}>
                        <span style={styles.mono}>EP{ep.order}</span> {ep.title}
                      </div>
                      {ep.description ? <div style={styles.cardSub}>{ep.description}</div> : null}
                    </div>
                    <button
                      style={styles.iconBtn}
                      onClick={(e) => {
                        e.stopPropagation()
                        handleDeleteEpisode(ep.id).catch(() => {})
                      }}
                      title="删除本集"
                    >
                      🗑️
                    </button>
                  </div>
                  <div style={{ display: 'flex', gap: 8, marginTop: 8 }}>
                    <button
                      style={styles.smallBtn}
                      onClick={(e) => {
                        e.stopPropagation()
                        openCreateSceneModal(ep.id)
                      }}
                    >
                      + 加场
                    </button>
                  </div>
                </div>
              ))}
              {episodes.length === 0 ? <div style={styles.empty}>暂无集，点击右上角“+ 新建集”</div> : null}
            </div>
          </div>

          <div style={styles.sidebarDivider} />

          {/* Scenes / Shots */}
          <div style={styles.sidebarSection}>
            <div style={styles.colHeader}>
              <div style={styles.colTitle}>{selectedScene ? 'Shots' : selectedEpisode ? 'Scenes' : 'Scenes'}</div>
            </div>
            <div style={styles.colScroll} className="aicomic-scroll">
              {/* Episode 选中且未选 Scene：显示 Scenes */}
              {selected.kind === 'episode' && selectedEpisode ? (
                <>
                  {(selectedEpisode.scenes || []).map((sc) => (
                    <div
                      key={sc.id}
                      style={{
                        ...styles.card,
                      }}
                      onClick={() => setSelected({ kind: 'scene', episodeId: selectedEpisode.id, sceneId: sc.id })}
                    >
                      <div style={styles.cardTitle}>
                        <span style={styles.mono}>SC{sc.sequence_number}</span> {sc.title}
                      </div>
                      {sc.description ? <div style={styles.cardSub}>{sc.description}</div> : null}
                    </div>
                  ))}
                  {(selectedEpisode.scenes || []).length === 0 ? <div style={styles.empty}>暂无场，点击上方 Episode 卡片的“+ 加场”</div> : null}
                </>
              ) : null}

              {/* Scene 选中：显示 Shots */}
              {selected.kind === 'scene' && selectedScene ? (
                <>
                  <div style={{ display: 'flex', gap: 8, marginBottom: 10 }}>
                    <button style={styles.btnPrimary} onClick={() => addShot(selectedScene.id).catch(() => {})}>
                      + 加镜
                    </button>
                    <button style={styles.btnDanger} onClick={() => handleDeleteScene(selectedScene.id).catch(() => {})}>
                      删除本场
                    </button>
                  </div>
                  {(selectedScene.shots || []).map((sh) => (
                    <div
                      key={sh.id}
                      style={{
                        ...styles.card,
                      }}
                      onClick={() =>
                        setSelected({
                          kind: 'shot',
                          episodeId: selected.episodeId,
                          sceneId: selectedScene.id,
                          shotId: sh.id,
                        })
                      }
                    >
                      <div style={{ display: 'flex', justifyContent: 'space-between', gap: 10 }}>
                        <div style={{ minWidth: 0 }}>
                          <div style={styles.cardTitle}>
                            <span style={styles.mono}>#{sh.sequence_number}</span> {sh.title || '（无标题）'}
                          </div>
                          {sh.action_text ? <div style={styles.cardSub}>{sh.action_text}</div> : null}
                        </div>
                        <button
                          style={styles.iconBtn}
                          title="删除镜头"
                          onClick={(e) => {
                            e.stopPropagation()
                            handleDeleteShot(sh.id).catch(() => {})
                          }}
                        >
                          ×
                        </button>
                      </div>
                    </div>
                  ))}
                  {(selectedScene.shots || []).length === 0 ? <div style={styles.empty}>暂无镜头，点击“+ 加镜”</div> : null}
                </>
              ) : null}

              {/* Shot 选中：仍然展示当前 scene 的 shots */}
              {selected.kind === 'shot' && selectedScene ? (
                <>
                  <div style={{ display: 'flex', gap: 8, marginBottom: 10 }}>
                    <button
                      style={styles.btn}
                      onClick={() => setSelected({ kind: 'scene', episodeId: selected.episodeId, sceneId: selectedScene.id })}
                    >
                      ← 返回本场
                    </button>
                  </div>
                  {(selectedScene.shots || []).map((sh) => (
                    <div
                      key={sh.id}
                      style={{
                        ...styles.card,
                        ...(selected.kind === 'shot' && selectedShot?.id === sh.id ? styles.cardActive : null),
                      }}
                      onClick={() =>
                        setSelected({
                          kind: 'shot',
                          episodeId: selected.episodeId,
                          sceneId: selectedScene.id,
                          shotId: sh.id,
                        })
                      }
                    >
                      <div style={styles.cardTitle}>
                        <span style={styles.mono}>#{sh.sequence_number}</span> {sh.title || '（无标题）'}
                      </div>
                      {sh.action_text ? <div style={styles.cardSub}>{sh.action_text}</div> : null}
                    </div>
                  ))}
                </>
              ) : null}

              {/* 未选择 episode 时 */}
              {selected.kind === 'none' ? <div style={styles.empty}>请选择左侧一集</div> : null}
            </div>
          </div>
        </div>

        {/* 右：详情编辑 */}
        <div style={styles.colRight}>
          <div style={styles.colHeader}>
            <div style={styles.colTitle}>详情</div>
          </div>
          <div style={styles.colScroll}>
            {/* Episode 编辑 */}
            {selected.kind === 'episode' && selectedEpisode ? (
              <section style={styles.panel}>
                <div style={styles.panelHeader}>
                  <div style={{ fontWeight: 700, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis' }}>
                    <span style={styles.mono}>EP{selectedEpisode.order}</span> {selectedEpisode.title}
                  </div>
                  <div style={{ display: 'flex', gap: 8 }}>
                    <button
                      style={styles.btn}
                      onClick={() => handleWorkflowScript().catch(() => {})}
                      disabled={workflowBusy !== null || (!episodeDescription.trim() && !workstationInput.trim())}
                      title="后端多代理工作流：架构师+编剧+QC+分场"
                    >
                      {workflowBusy === 'script' ? 'Workflow中…' : 'Workflow剧本'}
                    </button>
                    <button
                      style={styles.btnPrimary}
                      onClick={() => handleApplyWorkflowScript().catch(() => {})}
                      disabled={workflowBusy !== null || lastWorkflowKind !== 'script' || !lastWorkflowRunId}
                      title="将上一次 Workflow 剧本写回 Episode.description（可选覆盖 scenes）"
                    >
                      {workflowBusy === 'apply_script' ? '应用中…' : '应用Workflow'}
                    </button>
                    <button style={styles.btnPrimary} onClick={() => saveEpisodeScript().catch(() => {})}>
                      保存剧本到云端
                    </button>
                  </div>
                </div>

                {/* Workstation Tabs */}
                <div style={{ display: 'flex', gap: 2, borderBottom: '1px solid rgba(255,255,255,0.1)' }}>
                  {EP_ACTIONS.map((a) => (
                    <button
                      key={a.key}
                      style={{
                        ...styles.tabBtn,
                        ...(epActionTab === a.key ? styles.tabBtnActive : {}),
                      }}
                      onClick={() => setEpActionTab(a.key)}
                    >
                      {a.title}
                    </button>
                  ))}
                </div>

                {/* Workflow 状态提示 */}
                {workflowError ? <div style={{ color: '#f87171', fontSize: 12, marginBottom: 8 }}>{workflowError}</div> : null}
                {lastWorkflowRunId && lastWorkflowKind === 'script' ? (
                  <div style={{ fontSize: 12, opacity: 0.8, marginBottom: 8 }}>
                    最近 Workflow run_id：{lastWorkflowRunId}
                  </div>
                ) : null}

                {/* Workstation Body: 新建区域 + 历史版本列表 */}
                <div style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
                  {/* 新建区域 */}
                  {/* 改为单栏宽布局 */}
                  <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
                    <div style={styles.labelRow}>
                      <div style={styles.label}>新建输入</div>
                      <button
                        style={styles.btnPrimary}
                        disabled={
                          aiWritingBusy !== null || isSplitting || workflowBusy !== null || !workstationInput.trim()
                        }
                        onClick={() => {
                          if (epActionTab === 'outline_generate') handleOutlineGenerate().catch(() => {})
                          else if (epActionTab === 'generate_script') handleGenerateScript().catch(() => {})
                          else if (epActionTab === 'script_optimize') handleScriptOptimize().catch(() => {})
                          else if (epActionTab === 'split_scenes') handleAutoSplit().catch(() => {})
                        }}
                      >
                        {aiWritingBusy || isSplitting || workflowBusy === 'script' ? '运行中...' : '开始执行'}
                      </button>
                    </div>
                    <textarea
                      value={workstationInput}
                      onChange={(e) => {
                        const v = e.target.value
                        setWorkstationDirty(true)
                        setWorkstationInput(v)
                        if (projectId && selected.kind === 'episode') {
                          const wsKey = _draftKeyWorkstation(projectId, selected.episodeId, epActionTab)
                          workstationDirtyKeyRef.current = wsKey
                          _lsSet(wsKey, v)
                        }
                      }}
                      style={{ ...styles.textarea, height: 160 }}
                      placeholder={
                        epActionTab === 'outline_generate' ? '输入故事概念、Logline...' :
                        epActionTab === 'generate_script' ? '输入大纲...' :
                        '输入剧本...'
                      }
                    />
                    {aiWritingError || splitError ? (
                      <div style={{ color: '#f87171', fontSize: 12, whiteSpace: 'pre-wrap' }}>
                        {typeof (aiWritingError || splitError) === 'object'
                          ? JSON.stringify(aiWritingError || splitError, null, 2)
                          : String(aiWritingError || splitError)}
                      </div>
                    ) : null}
                  </div>

                  {/* 历史版本区域 */}
                  <div style={{ borderTop: '1px solid rgba(255,255,255,0.1)', paddingTop: 12 }}>
                    <div style={{ ...styles.labelRow, marginBottom: 8 }}>
                      <div style={styles.label}>
                        历史版本 ({(epRuns[epActionTab] || []).length} 条)
                      </div>
                      <div style={{ fontSize: 11, opacity: 0.5 }}>滚动查看更多</div>
                    </div>

                    {/* Split Scenes 特殊处理 */}
                    {epActionTab === 'split_scenes' ? (
                      <div className="aicomic-scroll" style={{ maxHeight: 400, overflowY: 'auto', background: 'rgba(0,0,0,0.15)', borderRadius: 8, padding: 12 }}>
                         {splitScenesPreview.length === 0 ? (
                          <div style={styles.empty}>暂无分场结果，请在上方输入剧本并点击「开始执行」</div>
                         ) : (
                           <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
                              <div style={{ padding: 4, fontSize: 12, opacity: 0.7 }}>
                                <button
                                  style={styles.smallBtn}
                                  disabled={isImporting}
                                  onClick={() => handleImportScenes().catch(() => {})}
                                >
                                  {isImporting ? '导入中...' : '一键创建场景'}
                                </button>
                                <label style={{ marginLeft: 8 }}><input type="checkbox" checked={overwriteOnImport} onChange={e => setOverwriteOnImport(e.target.checked)} /> 覆盖</label>
                              </div>
                              {splitScenesPreview.map((sc) => (
                                <div key={sc._key} style={{ padding: 8, background: 'rgba(255,255,255,0.05)', borderRadius: 6 }}>
                                  <div style={{ fontWeight: 700, fontSize: 12 }}>{sc.title}</div>
                                  <div style={{ fontSize: 12, opacity: 0.8 }}>{sc.description}</div>
                                </div>
                              ))}
                           </div>
                         )}
                      </div>
                    ) : (
                      /* 历史版本列表 - 双列布局（最新的在上面） */
                      <div className="aicomic-scroll" style={{ maxHeight: 400, overflowY: 'auto' }}>
                        {(epRuns[epActionTab] || []).length === 0 ? (
                          <div style={{ ...styles.empty, padding: 32 }}>暂无历史记录，请在上方输入内容并点击「开始执行」</div>
                        ) : (
                          <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
                            {(epRuns[epActionTab] || []).slice().sort((a, b) => a.id - b.id).map((run, idx) => (
                              <div
                                key={run.id}
                                style={{
                                  display: 'grid',
                                  gridTemplateColumns: '1fr 1fr',
                                  gap: 12,
                                  background: 'rgba(0,0,0,0.15)',
                                  borderRadius: 8,
                                  padding: 12,
                                  border: '1px solid rgba(255,255,255,0.05)',
                                }}
                              >
                                {/* Left: Input */}
                                <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                                  <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                                    <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                                      <div style={{ fontSize: 11, fontWeight: 600, color: 'rgba(255,255,255,0.6)' }}>
                                        #{idx + 1} · {run.created_at.slice(0, 16).replace('T', ' ')}
                                      </div>
                                      <button
                                        style={{ ...styles.smallBtn, fontSize: 9, padding: '1px 5px', background: 'rgba(239,68,68,0.2)', color: '#f87171' }}
                                        onClick={() => deleteEpRun(run.id, epActionTab)}
                                        title="删除此条记录"
                                      >
                                        删除
                                      </button>
                                    </div>
                                    <div style={{ fontSize: 10, opacity: 0.4 }}>输入</div>
                                  </div>
                                  <div
                                    className="aicomic-scroll"
                                    style={{
                                      flex: 1,
                                      background: 'rgba(0,0,0,0.2)',
                                      borderRadius: 6,
                                      padding: 8,
                                      fontSize: 12,
                                      lineHeight: 1.5,
                                      maxHeight: 150,
                                      overflowY: 'auto',
                                      whiteSpace: 'pre-wrap',
                                      wordBreak: 'break-word',
                                    }}
                                  >
                                    {run.input_text || '(无输入)'}
                                  </div>
                                </div>
                                {/* Right: Output */}
                                <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                                  <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                                    <div style={{ fontSize: 10, opacity: 0.4 }}>输出</div>
                                    <div style={{ display: 'flex', gap: 6 }}>
                                      {epActionTab === 'outline_generate' && (
                                        <button
                                          style={{ ...styles.smallBtn, fontSize: 10, padding: '2px 6px' }}
                                          onClick={() => {
                                            const raw = run.output_text || ''
                                            setOutlinePreview({ raw, parsed: tryParseJsonObject(raw) })
                                            setOutlinePreviewMode('preview')
                                          }}
                                          title="预览大纲"
                                        >
                                          预览
                                        </button>
                                      )}
                                      {/* 大纲操作只有预览，剧本操作才有应用 */}
                                      {(epActionTab === 'generate_script' || epActionTab === 'script_optimize') && (
                                        <button
                                          style={{ ...styles.smallBtn, fontSize: 10, padding: '2px 6px' }}
                                          onClick={() => applyEpRunToEditor(run)}
                                          title="应用到下方 Master 编辑器"
                                        >
                                          应用
                                        </button>
                                      )}
                                    </div>
                                  </div>
                                  <div
                                    className="aicomic-scroll"
                                    style={{
                                      flex: 1,
                                      background: 'rgba(0,0,0,0.2)',
                                      borderRadius: 6,
                                      padding: 8,
                                      fontSize: 12,
                                      lineHeight: 1.5,
                                      maxHeight: 150,
                                      overflowY: 'auto',
                                      whiteSpace: 'pre-wrap',
                                      wordBreak: 'break-word',
                                    }}
                                  >
                                    {sanitizeOutputForDisplay(run.output_text || '')}
                                  </div>
                                  {run.output_text && looksLikeTruncatedJson(run.output_text) ? (
                                    <div style={{ color: '#fbbf24', fontSize: 10 }}>
                                      提示：输出可能被截断，请调大 max_tokens
                      </div>
                    ) : null}
                  </div>
                              </div>
                            ))}
                          </div>
                        )}
                      </div>
                    )}
                  </div>

                  {/* Chat 面板：版本记录下方，最终剧本上方 */}
                  {(epActionTab === 'outline_generate' ||
                    epActionTab === 'generate_script' ||
                    epActionTab === 'script_optimize') && (
                    <div style={{ borderTop: '1px solid rgba(255,255,255,0.1)', paddingTop: 12 }}>
                      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 8 }}>
                        <div style={{ fontSize: 12, fontWeight: 700, opacity: 0.85 }}>Chat（对话驱动优化）</div>
                        <label style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: 11, opacity: 0.75 }}>
                          <input type="checkbox" checked={chatDebug} onChange={(e) => setChatDebug(e.target.checked)} />
                          Debug
                        </label>
                      </div>
                      {chatError ? <div style={{ color: '#f87171', fontSize: 12, marginBottom: 8 }}>{chatError}</div> : null}

                      <div
                        className="aicomic-scroll"
                        style={{
                          maxHeight: 180,
                          overflowY: 'auto',
                          background: 'rgba(0,0,0,0.12)',
                          border: '1px solid rgba(255,255,255,0.06)',
                          borderRadius: 10,
                          padding: 10,
                          marginBottom: 10,
                        }}
                      >
                        {chatMsgs.length === 0 ? (
                          <div style={{ fontSize: 12, opacity: 0.5 }}>在这里描述你的修改意图，例如：“把对白更口语，节奏更快，减少旁白”。</div>
                        ) : (
                          <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
                            {chatMsgs.slice(-30).map((m) => (
                              <div key={m.id} style={{ display: 'flex', gap: 8, alignItems: 'flex-start' }}>
                                <div style={{ width: 48, fontSize: 11, opacity: 0.6 }}>{m.role === 'user' ? '用户' : '系统'}</div>
                                <div style={{ flex: 1, whiteSpace: 'pre-wrap', fontSize: 12, lineHeight: 1.5, opacity: 0.92 }}>
                                  {m.content}
                                </div>
                              </div>
                            ))}
                          </div>
                        )}
                      </div>

                      <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
                        <input
                          value={chatInput}
                          onChange={(e) => setChatInput(e.target.value)}
                          onKeyDown={(e) => {
                            if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) sendChat().catch(() => {})
                          }}
                          placeholder="输入你的意图（Ctrl/Cmd + Enter 发送）"
                          style={{ ...styles.select, flex: 1, padding: '10px 12px' }}
                          disabled={!projectId || selected.kind !== 'episode' || chatBusy}
                        />
                        <button style={styles.btnPrimary} onClick={() => sendChat().catch(() => {})} disabled={chatBusy || !chatInput.trim()}>
                          {chatBusy ? '执行中…' : '发送'}
                        </button>
                      </div>

                      {/* Debug 面板（全量） */}
                      {chatDebug && lastChatDebug ? (
                        <details style={{ marginTop: 10, background: 'rgba(0,0,0,0.12)', borderRadius: 10, padding: 10 }}>
                          <summary style={{ cursor: 'pointer', fontSize: 12, opacity: 0.8 }}>Debug Trace（展开查看 plan / steps / prompts / memory / raw）</summary>
                          <div style={{ marginTop: 10, display: 'flex', flexDirection: 'column', gap: 10 }}>
                            <div>
                              <div style={{ fontSize: 11, opacity: 0.6, marginBottom: 6 }}>Plan</div>
                              <textarea readOnly value={JSON.stringify(lastChatDebug.plan || null, null, 2)} style={{ ...styles.textarea, height: 160, fontSize: 11, fontFamily: 'monospace' }} />
                            </div>
                            <div>
                              <div style={{ fontSize: 11, opacity: 0.6, marginBottom: 6 }}>Steps Trace</div>
                              <textarea readOnly value={JSON.stringify(lastChatDebug.steps_trace || null, null, 2)} style={{ ...styles.textarea, height: 160, fontSize: 11, fontFamily: 'monospace' }} />
                            </div>
                            <div>
                              <div style={{ fontSize: 11, opacity: 0.6, marginBottom: 6 }}>Memory Trace</div>
                              <textarea readOnly value={JSON.stringify(lastChatDebug.memory_trace || null, null, 2)} style={{ ...styles.textarea, height: 160, fontSize: 11, fontFamily: 'monospace' }} />
                            </div>
                            <div>
                              <div style={{ fontSize: 11, opacity: 0.6, marginBottom: 6 }}>Planner Prompt</div>
                              <textarea readOnly value={String(lastChatDebug.planner_prompt || '')} style={{ ...styles.textarea, height: 180, fontSize: 11, fontFamily: 'monospace' }} />
                            </div>
                            <div>
                              <div style={{ fontSize: 11, opacity: 0.6, marginBottom: 6 }}>Planner Raw</div>
                              <textarea readOnly value={String(lastChatDebug.planner_raw || '')} style={{ ...styles.textarea, height: 140, fontSize: 11, fontFamily: 'monospace' }} />
                            </div>
                          </div>
                        </details>
                      ) : null}
                    </div>
                  )}
                </div>

                <div style={{ borderTop: '1px solid rgba(255,255,255,0.1)', margin: '12px 0' }} />

                {/* Master Editor */}
                <div style={styles.labelRow}>
                  <div style={styles.label}>最终剧本 (Master Script)</div>
                  <div style={{ fontSize: 12, opacity: 0.5 }}>此处内容对应数据库存储</div>
                </div>
                <textarea
                  value={episodeDescription}
                  onChange={(e) => {
                    const v = e.target.value
                    setEpisodeDirty(true)
                    setEpisodeDescription(v)
                    if (projectId && selected.kind === 'episode') {
                      episodeDirtyForIdRef.current = selected.episodeId
                      _lsSet(_draftKeyEpisode(projectId, selected.episodeId), v)
                    }
                  }}
                  style={{ ...styles.textarea, height: 300 }}
                  placeholder="本集的最终剧本内容..."
                />
              </section>
            ) : null}

            {/* Scene 编辑 */}
            {selected.kind === 'scene' && selectedScene ? (
              <section style={styles.panel}>
                <div style={styles.panelHeader}>
                  <div style={{ fontWeight: 700, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis' }}>
                    <span style={styles.mono}>SC{selectedScene.sequence_number}</span> {selectedScene.title}
                  </div>
                  <button style={styles.btnPrimary} onClick={() => saveSceneScript().catch(() => {})}>
                    保存修改
                  </button>
                </div>

                <div style={styles.labelRow}>
                  <div style={styles.label}>剧本内容</div>
                  <div style={{ display: 'flex', gap: 8, alignItems: 'center', flexWrap: 'wrap' }}>
                    <select
                      value={storyboardPromptStyle}
                      onChange={(e) => setStoryboardPromptStyle(e.target.value as 'sd_tags' | 'mj_v6')}
                      style={{ ...styles.select, fontSize: 11, padding: '6px 8px' }}
                      title="提示词风格"
                    >
                      <option value="sd_tags">SD/Flux Tags</option>
                      <option value="mj_v6">Midjourney v6</option>
                    </select>
                    {storyboardPromptStyle === 'mj_v6' ? (
                      <input
                        type="text"
                        value={storyboardAspectRatio}
                        onChange={(e) => setStoryboardAspectRatio(e.target.value)}
                        placeholder="--ar 16:9"
                        style={{ ...styles.input, width: 100, fontSize: 11, padding: '6px 8px' }}
                        title="Aspect Ratio (如 16:9, 9:16, 2:3)"
                      />
                    ) : null}
                    <button
                      style={styles.btn}
                      onClick={() => handleAutoStoryboard().catch(() => {})}
                      disabled={isStoryboardSplitting || !sceneDescription.trim()}
                      title="自动分镜"
                    >
                      {isStoryboardSplitting ? '分镜中…' : '自动分镜'}
                    </button>
                    <button
                      style={styles.btn}
                      onClick={() => handleWorkflowStoryboard().catch(() => {})}
                      disabled={workflowBusy !== null || !sceneDescription.trim()}
                      title="后端多代理工作流：分镜 + prompt 翻译 + run 快照"
                    >
                      {workflowBusy === 'storyboard' ? 'Workflow中…' : 'Workflow分镜'}
                    </button>
                    <button
                      style={styles.btnPrimary}
                      onClick={() => handleApplyWorkflowStoryboard().catch(() => {})}
                      disabled={workflowBusy !== null || lastWorkflowKind !== 'storyboard' || !lastWorkflowRunId}
                      title="将上一次 Workflow 分镜写回 Shot.action_text/prompt/negative_prompt"
                    >
                      {workflowBusy === 'apply_storyboard' ? '应用中…' : '应用Workflow'}
                    </button>
                  </div>
                </div>

                <textarea
                  value={sceneDescription}
                  onChange={(e) => {
                    const v = e.target.value
                    setSceneDirty(true)
                    setSceneDescription(v)
                    if (projectId && selected.kind === 'scene') {
                      sceneDirtyForIdRef.current = selected.sceneId
                      _lsSet(_draftKeyScene(projectId, selected.sceneId), v)
                    }
                  }}
                  style={styles.textarea}
                  placeholder="编写本场的剧本内容…"
                />

                <div style={{ marginTop: 12 }}>
                  {workflowError ? <div style={{ color: '#f87171', fontSize: 12, marginBottom: 8 }}>{workflowError}</div> : null}
                  {lastWorkflowRunId && lastWorkflowKind === 'storyboard' ? (
                    <div style={{ fontSize: 12, opacity: 0.8, marginBottom: 8 }}>最近 Workflow run_id：{lastWorkflowRunId}</div>
                  ) : null}
                  <div style={styles.labelRow}>
                    <div style={styles.label}>分镜预览 {storyboardPreview.length ? `(${storyboardPreview.length} 镜)` : ''}</div>
                    <label style={{ fontSize: 12, opacity: 0.8, display: 'flex', alignItems: 'center', gap: 6 }}>
                      <input
                        type="checkbox"
                        checked={overwriteShotsOnImport}
                        onChange={(e) => setOverwriteShotsOnImport(e.target.checked)}
                      />
                      覆盖导入（先清空本场已有镜头）
                    </label>
                    <button
                      style={styles.btnPrimary}
                      onClick={() => handleImportShots().catch(() => {})}
                      disabled={isStoryboardImporting || storyboardPreview.length === 0}
                      title="一键导入为镜头"
                    >
                      {isStoryboardImporting ? '导入中…' : '一键导入'}
                    </button>
                  </div>
                  {storyboardError ? <div style={{ color: '#f87171', fontSize: 12 }}>{storyboardError}</div> : null}
                  {storyboardPreview.length === 0 ? (
                    <div style={styles.emptyBox}>点击“自动分镜”生成预览，然后点击“一键导入”创建镜头。</div>
                  ) : (
                    <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
                      {storyboardPreview.map((sh, idx) => (
                        <div key={sh._key} style={styles.previewCard}>
                          <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 8 }}>
                            <span style={styles.miniMono}>SHOT{idx + 1}</span>
                            <input
                              value={sh.title || ''}
                              onChange={(e) => {
                                const v = e.target.value
                                setStoryboardPreview((prev) => prev.map((x) => (x._key === sh._key ? { ...x, title: v } : x)))
                              }}
                              style={styles.input}
                              placeholder="镜头标题（可选）"
                            />
                          </div>
                          <textarea
                            value={sh.action_text}
                            onChange={(e) => {
                              const v = e.target.value
                              setStoryboardPreview((prev) =>
                                prev.map((x) => (x._key === sh._key ? { ...x, action_text: v } : x)),
                              )
                            }}
                            style={{ ...styles.textarea, height: 120 }}
                            placeholder="镜头描述…"
                          />
                          {sh.prompt ? (
                            <div style={{ marginTop: 8 }}>
                              <div style={{ fontSize: 11, opacity: 0.7, marginBottom: 4 }}>Prompt:</div>
                              <div style={{ fontSize: 11, padding: 6, background: 'rgba(99,102,241,0.1)', borderRadius: 6, fontFamily: 'monospace' }}>
                                {sh.prompt}
                              </div>
                            </div>
                          ) : null}
                          {sh.negative_prompt ? (
                            <div style={{ marginTop: 8 }}>
                              <div style={{ fontSize: 11, opacity: 0.7, marginBottom: 4 }}>Negative Prompt:</div>
                              <div style={{ fontSize: 11, padding: 6, background: 'rgba(248,113,113,0.1)', borderRadius: 6, fontFamily: 'monospace' }}>
                                {sh.negative_prompt}
                              </div>
                            </div>
                          ) : null}
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              </section>
            ) : null}

            {/* Shot 编辑 */}
            {selected.kind === 'shot' && shotDraft ? (
              <section style={styles.panel}>
                <div style={styles.panelHeader}>
                  <div style={{ fontWeight: 700 }}>
                    <span style={styles.mono}>SHOT</span> #{shotDraft.sequence_number}
                  </div>
                  <button style={styles.btnPrimary} onClick={() => saveShot().catch(() => {})}>
                    保存镜头
                  </button>
                </div>

                <div style={{ display: 'grid', gridTemplateColumns: '1fr 160px', gap: 10 }}>
                  <div>
                    <div style={styles.label}>标题</div>
                    <input
                      value={shotDraft.title || ''}
                      onChange={(e) => setShotDraft((p) => ({ ...(p || {}), title: e.target.value }))}
                      style={styles.input}
                      placeholder="镜头标题"
                    />
                  </div>
                  <div>
                    <div style={styles.label}>状态</div>
                    <select
                      value={shotDraft.status || 'draft'}
                      onChange={(e) => setShotDraft((p) => ({ ...(p || {}), status: e.target.value }))}
                      style={styles.select}
                    >
                      <option value="draft">draft</option>
                      <option value="todo">todo</option>
                      <option value="doing">doing</option>
                      <option value="done">done</option>
                    </select>
                  </div>
                </div>

                <div style={{ marginTop: 10 }}>
                  <div style={styles.label}>动作文本</div>
                  <textarea
                    value={shotDraft.action_text || ''}
                    onChange={(e) => setShotDraft((p) => ({ ...(p || {}), action_text: e.target.value }))}
                    style={styles.textarea}
                    placeholder="镜头动作/画面描述…"
                  />
                </div>
                <div style={{ marginTop: 10 }}>
                  <div style={styles.label}>对白</div>
                  <textarea
                    value={shotDraft.dialogue || ''}
                    onChange={(e) => setShotDraft((p) => ({ ...(p || {}), dialogue: e.target.value }))}
                    style={{ ...styles.textarea, height: 120 }}
                    placeholder="对白…"
                  />
                </div>
                <div style={{ marginTop: 10 }}>
                  <div style={styles.label}>Prompt</div>
                  <textarea
                    value={shotDraft.prompt || ''}
                    onChange={(e) => setShotDraft((p) => ({ ...(p || {}), prompt: e.target.value }))}
                    style={{ ...styles.textarea, height: 120 }}
                    placeholder="生成提示词…"
                  />
                </div>
              </section>
            ) : null}

            {selected.kind === 'none' ? <div style={styles.empty}>从左侧选择一集开始。</div> : null}
          </div>
        </div>
      </div>

      {/* AI 结果预览弹窗（应用到文本前先预览） */}
      {aiResult ? (
        <div style={styles.modalMask} onClick={() => setAiResult(null)}>
          <div style={styles.modal} onClick={(e) => e.stopPropagation()}>
            <div style={{ display: 'flex', justifyContent: 'space-between', gap: 10, alignItems: 'center', marginBottom: 10 }}>
              <div style={{ fontWeight: 900 }}>{aiResult.title}</div>
              <button style={styles.btn} onClick={() => setAiResult(null)}>
                关闭
              </button>
            </div>
            <textarea value={sanitizeOutputForDisplay(aiResult.text)} readOnly style={{ ...styles.textarea, height: 420 }} />
            <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 10, marginTop: 10 }}>
              <button
                style={styles.btn}
                onClick={() => {
                  navigator.clipboard?.writeText(aiResult.text || '').catch(() => {})
                }}
              >
                复制
              </button>
              <button
                style={styles.btnPrimary}
                onClick={() => {
                  setEpisodeDescription(aiResult.text || '')
                  setAiResult(null)
                }}
              >
                应用到当前文本
              </button>
            </div>
          </div>
        </div>
      ) : null}

      {/* 大纲生成：精美预览弹窗 */}
      {outlinePreview ? (
        <div style={styles.modalMask} onClick={() => setOutlinePreview(null)}>
          <div
            style={{ ...styles.modal, width: 980, maxWidth: '95vw' }}
            onClick={(e) => e.stopPropagation()}
          >
            <div style={{ display: 'flex', justifyContent: 'space-between', gap: 10, alignItems: 'center', marginBottom: 10 }}>
              <div style={{ fontWeight: 900 }}>
                {(() => {
                  const t = outlinePreview.parsed ? pickFirst(outlinePreview.parsed, ['title', 'name', '作品名']) : null
                  return t ? `大纲预览：${String(t)}` : '大纲预览'
                })()}
              </div>
              <div style={{ display: 'flex', gap: 8 }}>
                <button
                  style={outlinePreviewMode === 'preview' ? styles.btnPrimary : styles.btn}
                  onClick={() => setOutlinePreviewMode('preview')}
                >
                  预览
                </button>
                <button
                  style={outlinePreviewMode === 'raw' ? styles.btnPrimary : styles.btn}
                  onClick={() => setOutlinePreviewMode('raw')}
                >
                  原始
                </button>
                <button
                  style={styles.btn}
                  onClick={() => {
                    navigator.clipboard?.writeText(stripMarkdownCodeFences(outlinePreview.raw) || '').catch(() => {})
                  }}
                >
                  复制原文
                </button>
                <button style={styles.btn} onClick={() => setOutlinePreview(null)}>
                  关闭
                </button>
              </div>
            </div>

            {outlinePreviewMode === 'raw' ? (
              <textarea
                readOnly
                value={sanitizeOutputForDisplay(outlinePreview.raw)}
                style={{ ...styles.textarea, height: '70vh', width: '100%' }}
              />
            ) : (
              /* 内容（预览模式） */
              <div style={{ display: 'grid', gridTemplateColumns: '460px 1fr', gap: 12 }}>
                {/* 左：总览 + 视觉基调 + 角色速览（避免大块空白） */}
                <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
                  <div style={{ border: '1px solid rgba(255,255,255,0.10)', borderRadius: 12, padding: 14, background: 'rgba(255,255,255,0.03)' }}>
                    <div style={{ fontWeight: 900, marginBottom: 8, fontSize: 14 }}>总览</div>
                    {outlinePreview.parsed ? (
                      (() => {
                        const o = outlinePreview.parsed
                        const title = pickFirst(o, ['title', 'name', '作品名'])
                        const logline = pickFirst(o, ['revised_logline', 'logline', 'Logline', '一句话梗概', '梗概', '概念', 'story_concept'])
                        const theme = pickFirst(o, ['theme', '主题'])
                        const tone = pickFirst(o, ['tone', '风格', '基调'])
                        const editorNotes = pickFirst(o, ['editor_notes', 'editorNotes', 'notes', '编辑备注'])
                        return (
                          <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
                            {title ? (
                              <div style={{ fontWeight: 900, fontSize: 18, lineHeight: 1.2 }}>{String(title)}</div>
                            ) : null}
                            <div style={{ padding: 10, borderRadius: 10, background: 'rgba(0,0,0,0.25)' }}>
                              <div style={{ fontSize: 12, opacity: 0.7, marginBottom: 6 }}>Logline</div>
                              <div style={{ whiteSpace: 'pre-wrap', lineHeight: 1.5 }}>{String(logline ?? '（未提供）')}</div>
                            </div>
                            {editorNotes && typeof editorNotes === 'object' ? (
                              <div style={{ padding: 10, borderRadius: 10, background: 'rgba(0,0,0,0.25)' }}>
                                <div style={{ fontSize: 12, opacity: 0.7, marginBottom: 6 }}>编辑备注</div>
                                <div style={{ whiteSpace: 'pre-wrap', lineHeight: 1.5 }}>
                                  {pickFirst(editorNotes, ['major_issues_fixed']) ? `修复要点：${String(pickFirst(editorNotes, ['major_issues_fixed']))}\n` : ''}
                                  {pickFirst(editorNotes, ['pacing_verdict']) ? `节奏结论：${String(pickFirst(editorNotes, ['pacing_verdict']))}` : ''}
                                </div>
                              </div>
                            ) : null}
                            {(theme || tone) ? (
                              <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 10 }}>
                                <div style={{ padding: 10, borderRadius: 10, background: 'rgba(0,0,0,0.25)' }}>
                                  <div style={{ fontSize: 12, opacity: 0.7, marginBottom: 6 }}>主题</div>
                                  <div style={{ whiteSpace: 'pre-wrap', lineHeight: 1.5 }}>{theme ? String(theme) : '—'}</div>
                                </div>
                                <div style={{ padding: 10, borderRadius: 10, background: 'rgba(0,0,0,0.25)' }}>
                                  <div style={{ fontSize: 12, opacity: 0.7, marginBottom: 6 }}>基调</div>
                                  <div style={{ whiteSpace: 'pre-wrap', lineHeight: 1.5 }}>{tone ? String(tone) : '—'}</div>
                                </div>
                              </div>
                            ) : null}
                          </div>
                        )
                      })()
                    ) : (
                      <div style={{ opacity: 0.85, lineHeight: 1.6, whiteSpace: 'pre-wrap' }}>
                        {sanitizeOutputForDisplay(outlinePreview.raw)}
                      </div>
                    )}
                  </div>

                  <div style={{ border: '1px solid rgba(255,255,255,0.10)', borderRadius: 12, padding: 14, background: 'rgba(255,255,255,0.03)' }}>
                    <div style={{ fontWeight: 900, marginBottom: 8, fontSize: 14 }}>视觉基调</div>
                    <div style={{ padding: 10, borderRadius: 10, background: 'rgba(0,0,0,0.25)', whiteSpace: 'pre-wrap', lineHeight: 1.55 }}>
                      {outlinePreview.parsed ? String(pickFirst(outlinePreview.parsed, ['visual_tone', 'visualTone', 'visual_style', 'visualStyle', '视觉风格']) ?? '—') : '—'}
                    </div>
                  </div>

                  <div style={{ border: '1px solid rgba(255,255,255,0.10)', borderRadius: 12, padding: 14, background: 'rgba(255,255,255,0.03)' }}>
                    <div style={{ fontWeight: 900, marginBottom: 8, fontSize: 14 }}>角色速览</div>
                    {outlinePreview.parsed ? (
                      (() => {
                        const chars = pickFirst(outlinePreview.parsed, ['characters', 'character_optimizations', 'characterOptimizations', '角色', '人物'])
                        if (Array.isArray(chars)) {
                          return (
                            <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
                              {chars.slice(0, 8).map((c, idx) => (
                                <div key={idx} style={{ padding: 10, borderRadius: 10, background: 'rgba(0,0,0,0.25)' }}>
                                  <div style={{ fontWeight: 800 }}>
                                    {String(pickFirst(c, ['name', '姓名', '角色名']) ?? `角色${idx + 1}`)}
                                    {pickFirst(c, ['role', '定位', '身份']) ? (
                                      <span style={{ marginLeft: 8, fontSize: 12, opacity: 0.7 }}>{String(pickFirst(c, ['role', '定位', '身份']))}</span>
                                    ) : null}
                                  </div>
                                  {pickFirst(c, ['core_conflict', 'coreConflict', 'conflict', '矛盾']) ? (
                                    <div style={{ marginTop: 6, fontSize: 12, opacity: 0.85, whiteSpace: 'pre-wrap', lineHeight: 1.5 }}>
                                      {String(pickFirst(c, ['core_conflict', 'coreConflict', 'conflict', '矛盾']))}
                                    </div>
                                  ) : null}
                                </div>
                              ))}
                              {chars.length > 8 ? <div style={{ fontSize: 12, opacity: 0.6 }}>… 还有 {chars.length - 8} 个角色</div> : null}
                            </div>
                          )
                        }
                        return <div style={{ opacity: 0.75, fontSize: 12 }}>未提供角色列表。</div>
                      })()
                    ) : (
                      <div style={{ opacity: 0.75, fontSize: 12 }}>（仅 JSON 输出时可结构化展示）</div>
                    )}
                  </div>
                </div>

                {/* 右：分幕 / 角色 */}
                <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
                <div style={{ border: '1px solid rgba(255,255,255,0.10)', borderRadius: 12, padding: 14, background: 'rgba(255,255,255,0.03)' }}>
                  <div style={{ fontWeight: 900, marginBottom: 8, fontSize: 14 }}>三幕结构</div>
                  {outlinePreview.parsed ? (
                    <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
                      {extractActs(outlinePreview.parsed).length ? (
                        extractActs(outlinePreview.parsed).map((a) => {
                          const beats = pickFirst(a.data, ['beats', '节拍', 'key_beats', 'keyBeats'])
                          const actHook = pickFirst(a.data, ['act_climax_hook', 'climax_hook', 'hook', '悬念', '钩子'])
                          const finalImage = pickFirst(a.data, ['final_image', 'ending_image', 'finalImage', '终幕画面', '结尾画面'])
                          const summary = pickFirst(a.data, ['summary', '梗概', '概要']) ?? (typeof a.data === 'string' ? a.data : '')
                          const climax = pickFirst(a.data, ['climax_visual', 'climax', '高潮', '高潮画面'])
                          const resolution = pickFirst(a.data, ['resolution', '结局', '收束', '结尾'])
                          const inciting = pickFirst(a.data, ['inciting_incident', 'incitingIncident', 'inciting', '导火索', '引爆点'])
                          const midpoint = pickFirst(a.data, ['midpoint', 'mid_point', 'midPoint', '中点'])
                          const allIsLost = pickFirst(a.data, ['all_is_lost_moment', 'allIsLostMoment', 'all_is_lost', 'allIsLost', '至暗时刻'])
                          const actBreakHook = pickFirst(a.data, ['act_break_hook', 'actBreakHook', 'break_hook', '转折钩子'])
                          const emoShift = pickFirst(a.data, ['emotional_shift', 'emotionalShift', '情绪转折', '情绪变化'])
                          return (
                            <div key={a.key} style={{ padding: 10, borderRadius: 10, background: 'rgba(0,0,0,0.25)' }}>
                              <div style={{ fontWeight: 800, marginBottom: 6 }}>{a.title}</div>
                              {/* 你的 schema：beats + hook/final_image */}
                              {Array.isArray(beats) ? (
                                <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
                                  {beats.slice(0, 12).map((b, idx) => {
                                    const beatName = pickFirst(b, ['beat_name', 'beatName', 'name', '节拍名', '节点']) ?? `Beat ${idx + 1}`
                                    const actionDesc = pickFirst(b, ['action_description', 'actionDescription', 'action', '动作', '画面']) ?? ''
                                    const emo = pickFirst(b, ['emotional_charge', 'emotionalCharge', 'emotion', '情绪', '情绪电荷'])
                                    const vf = pickFirst(b, ['visual_focus', 'visualFocus', 'focus', '视觉重点', '关键画面'])
                                    return (
                                      <div key={idx} style={{ padding: 8, borderRadius: 10, background: 'rgba(255,255,255,0.04)' }}>
                                        <div style={{ fontWeight: 800, marginBottom: 4, fontSize: 12 }}>{String(beatName)}</div>
                                        {actionDesc ? <div style={{ whiteSpace: 'pre-wrap', lineHeight: 1.45, opacity: 0.9 }}>{String(actionDesc)}</div> : null}
                                        {(emo || vf) ? (
                                          <div style={{ marginTop: 6, display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 10, fontSize: 12, opacity: 0.85 }}>
                                            <div>情绪：{emo ? String(emo) : '—'}</div>
                                            <div>视觉：{vf ? String(vf) : '—'}</div>
                                          </div>
                                        ) : null}
                                      </div>
                                    )
                                  })}
                                  {(actHook || finalImage) ? (
                                    <div style={{ marginTop: 2, display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 10 }}>
                                      <div style={{ padding: 8, borderRadius: 10, background: 'rgba(255,255,255,0.04)' }}>
                                        <div style={{ fontSize: 12, opacity: 0.7, marginBottom: 4 }}>幕末钩子</div>
                                        <div style={{ whiteSpace: 'pre-wrap', lineHeight: 1.45 }}>{actHook ? String(actHook) : '—'}</div>
                                      </div>
                                      <div style={{ padding: 8, borderRadius: 10, background: 'rgba(255,255,255,0.04)' }}>
                                        <div style={{ fontSize: 12, opacity: 0.7, marginBottom: 4 }}>终幕画面</div>
                                        <div style={{ whiteSpace: 'pre-wrap', lineHeight: 1.45 }}>{finalImage ? String(finalImage) : '—'}</div>
                                      </div>
                                    </div>
                                  ) : null}
                                </div>
                              ) : (
                                <>
                                  {summary ? <div style={{ whiteSpace: 'pre-wrap', lineHeight: 1.5 }}>{String(summary)}</div> : null}
                                  {(inciting || midpoint || allIsLost || actBreakHook || emoShift) ? (
                                    <div style={{ marginTop: 8, display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 10 }}>
                                      {inciting ? (
                                        <div style={{ padding: 8, borderRadius: 10, background: 'rgba(255,255,255,0.04)' }}>
                                          <div style={{ fontSize: 12, opacity: 0.7, marginBottom: 4 }}>引爆点</div>
                                          <div style={{ whiteSpace: 'pre-wrap', lineHeight: 1.45 }}>{String(inciting)}</div>
                                        </div>
                                      ) : null}
                                      {midpoint ? (
                                        <div style={{ padding: 8, borderRadius: 10, background: 'rgba(255,255,255,0.04)' }}>
                                          <div style={{ fontSize: 12, opacity: 0.7, marginBottom: 4 }}>中点</div>
                                          <div style={{ whiteSpace: 'pre-wrap', lineHeight: 1.45 }}>{String(midpoint)}</div>
                                        </div>
                                      ) : null}
                                      {allIsLost ? (
                                        <div style={{ padding: 8, borderRadius: 10, background: 'rgba(255,255,255,0.04)' }}>
                                          <div style={{ fontSize: 12, opacity: 0.7, marginBottom: 4 }}>至暗时刻</div>
                                          <div style={{ whiteSpace: 'pre-wrap', lineHeight: 1.45 }}>{String(allIsLost)}</div>
                                        </div>
                                      ) : null}
                                      {actBreakHook ? (
                                        <div style={{ padding: 8, borderRadius: 10, background: 'rgba(255,255,255,0.04)' }}>
                                          <div style={{ fontSize: 12, opacity: 0.7, marginBottom: 4 }}>幕末钩子</div>
                                          <div style={{ whiteSpace: 'pre-wrap', lineHeight: 1.45 }}>{String(actBreakHook)}</div>
                                        </div>
                                      ) : null}
                                      {emoShift ? (
                                        <div style={{ padding: 8, borderRadius: 10, background: 'rgba(255,255,255,0.04)' }}>
                                          <div style={{ fontSize: 12, opacity: 0.7, marginBottom: 4 }}>情绪转折</div>
                                          <div style={{ whiteSpace: 'pre-wrap', lineHeight: 1.45 }}>{String(emoShift)}</div>
                                        </div>
                                      ) : null}
                                    </div>
                                  ) : null}
                                  {(climax || resolution) ? (
                                    <div style={{ marginTop: 8, display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 10 }}>
                                      <div style={{ padding: 8, borderRadius: 10, background: 'rgba(255,255,255,0.04)' }}>
                                        <div style={{ fontSize: 12, opacity: 0.7, marginBottom: 4 }}>高潮画面</div>
                                        <div style={{ whiteSpace: 'pre-wrap', lineHeight: 1.45 }}>{climax ? String(climax) : '—'}</div>
                                      </div>
                                      <div style={{ padding: 8, borderRadius: 10, background: 'rgba(255,255,255,0.04)' }}>
                                        <div style={{ fontSize: 12, opacity: 0.7, marginBottom: 4 }}>收束</div>
                                        <div style={{ whiteSpace: 'pre-wrap', lineHeight: 1.45 }}>{resolution ? String(resolution) : '—'}</div>
                                      </div>
                                    </div>
                                  ) : null}
                                </>
                              )}
                            </div>
                          )
                        })
                      ) : (
                        <div style={{ opacity: 0.75, fontSize: 12 }}>未识别到 Act1/Act2/Act3 字段（可在 Prompt 模板里统一成 JSON schema 以获得最佳预览）。</div>
                      )}
                    </div>
                  ) : (
                    <div style={{ opacity: 0.75, fontSize: 12 }}>当前输出不是 JSON，将以文本方式展示（可在 Prompt 模板中让大纲生成输出 JSON）。</div>
                  )}
                </div>

                <div style={{ border: '1px solid rgba(255,255,255,0.10)', borderRadius: 12, padding: 14, background: 'rgba(255,255,255,0.03)' }}>
                  <div style={{ fontWeight: 900, marginBottom: 8, fontSize: 14 }}>角色</div>
                  {outlinePreview.parsed ? (
                    (() => {
                      const chars = pickFirst(outlinePreview.parsed, ['character_optimizations', 'characterOptimizations', 'characters', '角色', '人物'])
                      if (Array.isArray(chars)) {
                        return (
                          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 10 }}>
                            {chars.slice(0, 12).map((c, idx) => {
                              const name = pickFirst(c, ['name', '姓名', '角色名']) ?? `角色${idx + 1}`
                              const role = pickFirst(c, ['role', '定位', '身份'])
                              const mv = pickFirst(c, ['motivation_visualized', 'motivationVisualized', 'visual', '视觉化动机'])
                              const visualDna = pickFirst(c, ['visual_dna', 'visualDna', 'dna', '视觉DNA'])
                              const coreConflict = pickFirst(c, ['core_conflict', 'coreConflict', 'conflict', '矛盾'])
                              const goal = pickFirst(c, ['goal', '目标'])
                              const conflict = pickFirst(c, ['conflict', '矛盾', '阻力'])
                              return (
                                <div key={idx} style={{ padding: 10, borderRadius: 10, background: 'rgba(0,0,0,0.25)' }}>
                                  <div style={{ fontWeight: 800, marginBottom: 6 }}>{String(name)}</div>
                                  <div style={{ fontSize: 12, opacity: 0.85, lineHeight: 1.5, whiteSpace: 'pre-wrap' }}>
                                    {role ? `身份：${String(role)}\n` : ''}
                                    {mv ? `动机（视觉化）：${String(mv)}\n` : ''}
                                    {visualDna ? `外观：${String(visualDna)}\n` : ''}
                                    {coreConflict ? `核心矛盾：${String(coreConflict)}\n` : ''}
                                    {goal ? `目标：${String(goal)}\n` : ''}
                                    {conflict ? `矛盾：${String(conflict)}` : ''}
                                  </div>
                                </div>
                              )
                            })}
                          </div>
                        )
                      }
                      if (chars && typeof chars === 'object') {
                        return (
                          <div style={{ opacity: 0.9, whiteSpace: 'pre-wrap', lineHeight: 1.6 }}>
                            {JSON.stringify(chars, null, 2)}
                          </div>
                        )
                      }
                      return <div style={{ opacity: 0.75, fontSize: 12 }}>未提供角色结构。</div>
                    })()
                  ) : (
                    <div style={{ opacity: 0.75, fontSize: 12 }}>（仅 JSON 输出时可结构化展示角色）</div>
                  )}
                </div>
              </div>
              </div>
            )}

            {/* 底部：截断提示 */}
            {looksLikeTruncatedJson(outlinePreview.raw) ? (
              <div style={{ color: '#fbbf24', fontSize: 12, marginTop: 10, whiteSpace: 'pre-wrap' }}>
                提示：该输出看起来像被截断（常见原因：max_tokens 太小）。如果你刚调到 8192 仍出现截断，建议提升到更高或拆分任务。
              </div>
            ) : null}
          </div>
        </div>
      ) : null}

      {/* 新建集弹窗 */}
      {showCreateEpisode ? (
        <div
          style={styles.modalMask}
          onClick={() => {
            if (creatingEpisode) return
            setShowCreateEpisode(false)
          }}
        >
          <div style={styles.modal} onClick={(e) => e.stopPropagation()}>
            <div style={{ display: 'flex', justifyContent: 'space-between', gap: 10, alignItems: 'center', marginBottom: 10 }}>
              <div style={{ fontWeight: 900 }}>新建一集</div>
              <button style={styles.btn} onClick={() => setShowCreateEpisode(false)} disabled={creatingEpisode}>
                关闭
              </button>
            </div>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
              <div style={{ fontSize: 12, opacity: 0.75 }}>将创建：EP{nextEpisodeOrder}</div>
              <input
                value={createEpisodeTitle}
                onChange={(e) => setCreateEpisodeTitle(e.target.value)}
                style={styles.input}
                placeholder="请输入标题"
                autoFocus
                onKeyDown={(e) => {
                  if (e.key === 'Enter') createEpisode().catch(() => {})
                }}
              />
              {createEpisodeError ? <div style={{ color: '#f87171', fontSize: 12 }}>{createEpisodeError}</div> : null}
            </div>
            <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 10, marginTop: 12 }}>
              <button style={styles.btn} onClick={() => setShowCreateEpisode(false)} disabled={creatingEpisode}>
                取消
              </button>
              <button style={styles.btnPrimary} onClick={() => createEpisode().catch(() => {})} disabled={creatingEpisode || !createEpisodeTitle.trim()}>
                {creatingEpisode ? '创建中…' : '创建'}
              </button>
            </div>
          </div>
        </div>
      ) : null}

      {/* 新建场弹窗 */}
      {showCreateScene ? (
        <div
          style={styles.modalMask}
          onClick={() => {
            if (creatingScene) return
            setShowCreateScene(false)
          }}
        >
          <div style={styles.modal} onClick={(e) => e.stopPropagation()}>
            <div style={{ display: 'flex', justifyContent: 'space-between', gap: 10, alignItems: 'center', marginBottom: 10 }}>
              <div style={{ fontWeight: 900 }}>新建一场</div>
              <button style={styles.btn} onClick={() => setShowCreateScene(false)} disabled={creatingScene}>
                关闭
              </button>
            </div>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
              <div style={{ fontSize: 12, opacity: 0.75 }}>
                将创建到：EP{selectedEpisode?.order ?? ''}{selectedEpisode?.title ? ` · ${selectedEpisode.title}` : ''}
              </div>
              <input
                value={createSceneTitle}
                onChange={(e) => setCreateSceneTitle(e.target.value)}
                style={styles.input}
                placeholder="请输入场景标题"
                autoFocus
                onKeyDown={(e) => {
                  if (e.key === 'Enter') createScene().catch(() => {})
                }}
              />
              {createSceneError ? <div style={{ color: '#f87171', fontSize: 12 }}>{createSceneError}</div> : null}
            </div>
            <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 10, marginTop: 12 }}>
              <button style={styles.btn} onClick={() => setShowCreateScene(false)} disabled={creatingScene}>
                取消
              </button>
              <button style={styles.btnPrimary} onClick={() => createScene().catch(() => {})} disabled={creatingScene || !createSceneTitle.trim()}>
                {creatingScene ? '创建中…' : '创建'}
              </button>
            </div>
          </div>
        </div>
      ) : null}

      {/* 新建项目弹窗 */}
      {showCreateProject ? (
        <div
          style={styles.modalMask}
          onClick={() => {
            if (creatingProject) return
            setShowCreateProject(false)
          }}
        >
          <div style={styles.modal} onClick={(e) => e.stopPropagation()}>
            <div style={{ display: 'flex', justifyContent: 'space-between', gap: 10, alignItems: 'center', marginBottom: 10 }}>
              <div style={{ fontWeight: 900 }}>新建项目</div>
              <button style={styles.btn} onClick={() => setShowCreateProject(false)} disabled={creatingProject}>
                关闭
              </button>
            </div>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
              <div>
                <div style={{ fontSize: 12, opacity: 0.75, marginBottom: 6 }}>项目名称</div>
                <input
                  value={createProjectName}
                  onChange={(e) => setCreateProjectName(e.target.value)}
                  style={styles.input}
                  placeholder="例如：第一季"
                  autoFocus
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') createProject().catch(() => {})
                  }}
                />
              </div>
              <div>
                <div style={{ fontSize: 12, opacity: 0.75, marginBottom: 6 }}>描述（可选）</div>
                <textarea
                  value={createProjectDesc}
                  onChange={(e) => setCreateProjectDesc(e.target.value)}
                  style={{ ...styles.textarea, height: 140 }}
                  placeholder="项目简介…"
                />
              </div>
              {createProjectError ? <div style={{ color: '#f87171', fontSize: 12 }}>{createProjectError}</div> : null}
            </div>
            <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 10, marginTop: 12 }}>
              <button style={styles.btn} onClick={() => setShowCreateProject(false)} disabled={creatingProject}>
                取消
              </button>
              <button style={styles.btnPrimary} onClick={() => createProject().catch(() => {})} disabled={creatingProject || !createProjectName.trim()}>
                {creatingProject ? '创建中…' : '创建'}
              </button>
            </div>
          </div>
        </div>
      ) : null}

      {/* 重命名项目弹窗 */}
      {showRenameProject ? (
        <div
          style={styles.modalMask}
          onClick={() => {
            if (renamingProject) return
            setShowRenameProject(false)
          }}
        >
          <div style={styles.modal} onClick={(e) => e.stopPropagation()}>
            <div style={{ display: 'flex', justifyContent: 'space-between', gap: 10, alignItems: 'center', marginBottom: 10 }}>
              <div style={{ fontWeight: 900 }}>重命名项目</div>
              <button style={styles.btn} onClick={() => setShowRenameProject(false)} disabled={renamingProject}>
                关闭
              </button>
            </div>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
              <div>
                <div style={{ fontSize: 12, opacity: 0.75, marginBottom: 6 }}>项目名称</div>
                <input
                  value={renameProjectName}
                  onChange={(e) => setRenameProjectName(e.target.value)}
                  style={styles.input}
                  placeholder="项目名称"
                  autoFocus
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') updateProjectMeta().catch(() => {})
                  }}
                />
              </div>
              <div>
                <div style={{ fontSize: 12, opacity: 0.75, marginBottom: 6 }}>描述（可选）</div>
                <textarea
                  value={renameProjectDesc}
                  onChange={(e) => setRenameProjectDesc(e.target.value)}
                  style={{ ...styles.textarea, height: 140 }}
                  placeholder="项目简介…"
                />
              </div>
              {renameProjectError ? <div style={{ color: '#f87171', fontSize: 12 }}>{renameProjectError}</div> : null}
            </div>
            <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 10, marginTop: 12 }}>
              <button style={styles.btn} onClick={() => setShowRenameProject(false)} disabled={renamingProject}>
                取消
              </button>
              <button
                style={styles.btnPrimary}
                onClick={() => updateProjectMeta().catch(() => {})}
                disabled={renamingProject || !renameProjectName.trim()}
              >
                {renamingProject ? '保存中…' : '保存'}
              </button>
            </div>
          </div>
        </div>
      ) : null}

      {/* 删除项目确认弹窗 */}
      {showDeleteProject ? (
        <div
          style={styles.modalMask}
          onClick={() => {
            if (deletingProject) return
            setShowDeleteProject(false)
          }}
        >
          <div style={styles.modal} onClick={(e) => e.stopPropagation()}>
            <div style={{ fontWeight: 900, marginBottom: 10 }}>删除项目</div>
            <div style={{ fontSize: 12, opacity: 0.85, lineHeight: 1.6 }}>
              将删除项目「{currentProject?.name || ''}」及其所有关联数据与本地文件夹。此操作不可撤销。
            </div>
            {deleteProjectError ? <div style={{ color: '#f87171', fontSize: 12, marginTop: 10 }}>{deleteProjectError}</div> : null}
            <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 10, marginTop: 14 }}>
              <button style={styles.btn} onClick={() => setShowDeleteProject(false)} disabled={deletingProject}>
                取消
              </button>
              <button style={styles.btnDanger} onClick={() => deleteCurrentProject().catch(() => {})} disabled={deletingProject}>
                {deletingProject ? '删除中…' : '确认删除'}
              </button>
            </div>
          </div>
        </div>
      ) : null}
    </div>
  )
}

const styles: Record<string, React.CSSProperties> = {
  page: {
    display: 'flex',
    flexDirection: 'column',
    gap: 12,
  },
  topbar: {
    display: 'flex',
    justifyContent: 'space-between',
    alignItems: 'center',
    padding: 12,
    borderRadius: 12,
    border: '1px solid rgba(255,255,255,0.08)',
    background: 'rgba(255,255,255,0.04)',
  },
  body: {
    display: 'grid',
    gridTemplateColumns: 'minmax(220px, 14%) 1fr',
    gap: 12,
    minHeight: 'calc(100vh - 140px)',
    height: 'calc(100vh - 140px)',
  },
  sidebar: {
    border: '1px solid rgba(255,255,255,0.08)',
    borderRadius: 12,
    overflow: 'hidden',
    background: 'rgba(255,255,255,0.03)',
    display: 'flex',
    flexDirection: 'column',
    minHeight: 0,
  },
  sidebarSection: {
    display: 'flex',
    flexDirection: 'column',
    flex: 1,
    minHeight: 0,
  },
  sidebarDivider: {
    height: 1,
    background: 'rgba(255,255,255,0.08)',
  },
  colRight: {
    border: '1px solid rgba(255,255,255,0.08)',
    borderRadius: 12,
    overflow: 'hidden',
    background: 'rgba(255,255,255,0.03)',
    display: 'flex',
    flexDirection: 'column',
    minHeight: 0,
  },
  colHeader: {
    padding: '10px 12px',
    borderBottom: '1px solid rgba(255,255,255,0.08)',
    background: 'rgba(0,0,0,0.12)',
  },
  colTitle: {
    fontWeight: 700,
    fontSize: 13,
    opacity: 0.9,
  },
  colScroll: {
    padding: 12,
    overflow: 'auto',
    flex: 1,
    minHeight: 0,
    scrollbarWidth: 'thin',
    scrollbarColor: 'rgba(255,255,255,0.2) rgba(0,0,0,0.1)',
  },
  card: {
    padding: 12,
    borderRadius: 12,
    border: '1px solid rgba(255,255,255,0.10)',
    background: 'rgba(0,0,0,0.18)',
    cursor: 'pointer',
    marginBottom: 10,
  },
  cardActive: {
    border: '1px solid rgba(99,102,241,0.8)',
    background: 'rgba(99,102,241,0.12)',
  },
  cardTitle: {
    fontWeight: 700,
    fontSize: 13,
    overflow: 'hidden',
    whiteSpace: 'nowrap',
    textOverflow: 'ellipsis',
  },
  cardSub: {
    fontSize: 12,
    opacity: 0.75,
    overflow: 'hidden',
    whiteSpace: 'nowrap',
    textOverflow: 'ellipsis',
    marginTop: 4,
  },
  panel: {
    display: 'flex',
    flexDirection: 'column',
    gap: 10,
  },
  panelHeader: {
    display: 'flex',
    justifyContent: 'space-between',
    alignItems: 'center',
    gap: 12,
  },
  labelRow: {
    display: 'flex',
    justifyContent: 'space-between',
    alignItems: 'center',
    gap: 10,
  },
  label: {
    fontSize: 12,
    opacity: 0.75,
    fontWeight: 700,
    textTransform: 'uppercase',
    letterSpacing: 0.5,
  },
  textarea: {
    width: '100%',
    height: 260,
    resize: 'vertical',
    borderRadius: 12,
    border: '1px solid rgba(255,255,255,0.10)',
    background: 'rgba(0,0,0,0.18)',
    color: '#e5e7eb',
    padding: 10,
    boxSizing: 'border-box',
    outline: 'none',
    scrollbarWidth: 'thin',
    scrollbarColor: 'rgba(255,255,255,0.2) rgba(0,0,0,0.1)',
  },
  input: {
    width: '100%',
    borderRadius: 10,
    border: '1px solid rgba(255,255,255,0.10)',
    background: 'rgba(0,0,0,0.18)',
    color: '#e5e7eb',
    padding: '8px 10px',
    outline: 'none',
    boxSizing: 'border-box',
  },
  select: {
    borderRadius: 10,
    border: '1px solid rgba(255,255,255,0.10)',
    background: 'rgba(0,0,0,0.18)',
    color: '#e5e7eb',
    padding: '8px 10px',
    outline: 'none',
  },
  btn: {
    borderRadius: 10,
    border: '1px solid rgba(255,255,255,0.14)',
    background: 'rgba(255,255,255,0.04)',
    color: '#e5e7eb',
    padding: '8px 10px',
    cursor: 'pointer',
  },
  btnPrimary: {
    borderRadius: 10,
    border: '1px solid rgba(99,102,241,0.6)',
    background: 'rgba(99,102,241,0.35)',
    color: '#fff',
    padding: '8px 10px',
    cursor: 'pointer',
  },
  btnDanger: {
    borderRadius: 10,
    border: '1px solid rgba(248,113,113,0.55)',
    background: 'rgba(248,113,113,0.20)',
    color: '#fff',
    padding: '8px 10px',
    cursor: 'pointer',
  },
  iconBtn: {
    border: 'none',
    background: 'transparent',
    color: '#cbd5e1',
    cursor: 'pointer',
  },
  smallBtn: {
    borderRadius: 10,
    border: '1px solid rgba(255,255,255,0.14)',
    background: 'rgba(255,255,255,0.04)',
    color: '#e5e7eb',
    padding: '6px 8px',
    cursor: 'pointer',
    fontSize: 12,
  },
  empty: {
    opacity: 0.6,
    fontSize: 13,
    padding: 10,
  },
  emptyBox: {
    opacity: 0.65,
    fontSize: 12,
    padding: 12,
    borderRadius: 12,
    border: '1px dashed rgba(255,255,255,0.18)',
  },
  previewCard: {
    padding: 12,
    borderRadius: 12,
    border: '1px solid rgba(255,255,255,0.10)',
    background: 'rgba(0,0,0,0.14)',
  },
  mono: {
    fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace',
    fontSize: 12,
    opacity: 0.75,
    marginRight: 8,
  },
  miniMono: {
    fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace',
    fontSize: 11,
    opacity: 0.65,
  },
  modalMask: {
    position: 'fixed',
    inset: 0,
    background: 'rgba(0,0,0,0.55)',
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    zIndex: 60,
  },
  modal: {
    width: 820,
    maxHeight: '86vh',
    overflow: 'auto',
    borderRadius: 12,
    border: '1px solid rgba(255,255,255,0.10)',
    background: '#0b1220',
    padding: 14,
  },
  tabBtn: {
    padding: '8px 16px',
    cursor: 'pointer',
    background: 'transparent',
    border: 'none',
    borderBottom: '2px solid transparent',
    color: '#94a3b8',
    fontSize: 13,
    fontWeight: 600,
  },
  tabBtnActive: {
    color: '#6366f1',
    borderBottom: '2px solid #6366f1',
  },
}



