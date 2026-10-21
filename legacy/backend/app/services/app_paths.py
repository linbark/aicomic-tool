import os


def data_dir() -> str:
    """
    与现有后端保持一致：
    - 由 AICOMIC_DATA_DIR 指定（桌面版/自定义）
    - 否则默认使用 CWD 下的 data/
    """
    return os.environ.get("AICOMIC_DATA_DIR") or os.path.join(os.getcwd(), "data")


def app_data_dir() -> str:
    """
    app_data_dir 是 data_dir 的父目录：
    - 避免被 FastAPI 的 /files 静态目录直接暴露（/files 只挂载 data_dir）
    - 复用现有 ai.py 的路径约定：ai_settings.json 与 prompt_templates.json 放在同级
    """
    return os.path.dirname(os.path.abspath(data_dir()))


def ai_settings_path() -> str:
    return os.path.join(app_data_dir(), "ai_settings.json")


def prompt_templates_path() -> str:
    return os.path.join(app_data_dir(), "prompt_templates.json")


def project_root_dir(project_id: int) -> str:
    return os.path.join(app_data_dir(), "projects", str(int(project_id)))


def project_context_dir(project_id: int) -> str:
    return os.path.join(project_root_dir(project_id), "context")


def project_runs_dir(project_id: int) -> str:
    return os.path.join(project_root_dir(project_id), "runs")


