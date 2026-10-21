from PIL import Image
import re
import os

def extract_metadata(file_path: str) -> dict:
    """
    解析图片文件，尝试提取 AI 生成信息 (针对 Stable Diffusion WebUI 格式)
    """
    if not os.path.exists(file_path):
        return {}

    try:
        img = Image.open(file_path)
        img.load() # 加载图片数据
    except Exception as e:
        print(f"Error opening image: {e}")
        return {}

    # 获取 PNG info 字典
    info = img.info
    
    # 初始化返回数据
    metadata = {
        "width": img.width,
        "height": img.height,
        "raw_parameters": "" # 保留原始文本作为备份
    }

    # Stable Diffusion WebUI 通常把信息存在 'parameters' 字段中
    if 'parameters' in info:
        raw_text = info['parameters']
        metadata['raw_parameters'] = raw_text
        
        # === 1. 分离 Prompt 和 Negative Prompt ===
        # WebUI 的格式通常是：Positive Prompt \nNegative prompt: ... \nSteps: ...
        parts = raw_text.split("Negative prompt:")
        
        if len(parts) > 1:
            metadata['prompt'] = parts[0].strip()
            # 剩下的部分包含 Negative prompt 和 参数块
            remaining = parts[1]
            
            # 参数块通常以 "Steps:" 开头，我们需要找到它切分
            if "Steps:" in remaining:
                neg_part, params_part = remaining.split("Steps:", 1)
                metadata['negative_prompt'] = neg_part.strip()
                params_text = "Steps:" + params_part # 把 Steps 补回去方便解析
            else:
                metadata['negative_prompt'] = remaining.strip()
                params_text = ""
        else:
            # 只有 Positive Prompt 或格式不标准
            metadata['prompt'] = parts[0].strip()
            params_text = ""
            
            # 尝试在没有 Negative prompt 的情况下找参数块
            if "Steps:" in parts[0]:
                 prompt_part, params_part = parts[0].split("Steps:", 1)
                 metadata['prompt'] = prompt_part.strip()
                 params_text = "Steps:" + params_part

        # === 2. 解析参数块 (使用正则提取键值对) ===
        # 典型格式: Steps: 20, Sampler: Euler a, CFG scale: 7, Seed: 12345, Size: 512x512, ...
        if params_text:
            # 提取 Seed
            seed_match = re.search(r"Seed: (\d+)", params_text)
            if seed_match:
                metadata['seed'] = int(seed_match.group(1))
            
            # 提取 Model Hash
            model_hash_match = re.search(r"Model hash: ([a-f0-9]+)", params_text)
            if model_hash_match:
                metadata['model_hash'] = model_hash_match.group(1)

            # 提取 Sampler
            sampler_match = re.search(r"Sampler: ([^,]+)", params_text)
            if sampler_match:
                metadata['sampler'] = sampler_match.group(1).strip()
            
            # 提取 CFG
            cfg_match = re.search(r"CFG scale: ([\d\.]+)", params_text)
            if cfg_match:
                metadata['cfg_scale'] = float(cfg_match.group(1))

    return metadata

# --- 测试代码 ---
if __name__ == "__main__":
    # 你可以在这里放一张你实际生成的图路径来测试
    test_path = "test_image.png" 
    print(extract_metadata(test_path))
