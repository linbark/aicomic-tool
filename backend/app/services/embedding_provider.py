"""
本地 Embedding 提供者（支持 bge-m3、gte-large-zh 等）
使用 ONNX/量化模型，CPU 优先
"""
from __future__ import annotations

import hashlib
from typing import TYPE_CHECKING, List, Optional

import numpy as np

if TYPE_CHECKING:  # pragma: no cover
    # 仅用于类型提示；运行时延迟导入以加速后端启动（sentence_transformers/torch import 很慢）
    from sentence_transformers import SentenceTransformer  # type: ignore


class EmbeddingProvider:
    """
    统一的 Embedding 提供者接口
    支持本地模型（CPU/ONNX），后续可替换为 GPU/云服务
    """

    def __init__(
        self,
        model_name: str = "BAAI/bge-m3",
        device: str = "cpu",
        cache_folder: Optional[str] = None,
    ):
        """
        初始化 Embedding 模型

        Args:
            model_name: 模型名称（默认 bge-m3，备选 gte-large-zh）
            device: 设备（cpu/cuda）
            cache_folder: 模型缓存目录（None 使用默认）
        """
        self.model_name = model_name
        self.device = device
        self._model: Optional["SentenceTransformer"] = None
        self._cache_folder = cache_folder

    def _get_model(self) -> "SentenceTransformer":
        """懒加载模型（并延迟导入 sentence_transformers 以加速服务启动）"""
        if self._model is None:
            import time
            import logging
            logger = logging.getLogger(__name__)
            t_start = time.time()
            logger.info(f"[EmbeddingProvider] Loading model '{self.model_name}' (device={self.device})...")
            from sentence_transformers import SentenceTransformer  # type: ignore
            self._model = SentenceTransformer(
                self.model_name,
                device=self.device,
                cache_folder=self._cache_folder,
            )
            t_end = time.time()
            logger.info(f"[EmbeddingProvider] Model '{self.model_name}' loaded in {t_end - t_start:.2f}s")
        return self._model

    def embed(self, texts: List[str], normalize: bool = True) -> np.ndarray:
        """
        对文本列表进行 embedding

        Args:
            texts: 文本列表
            normalize: 是否归一化（L2 归一化，便于余弦相似度计算）

        Returns:
            numpy array，shape=(len(texts), embedding_dim)
        """
        if not texts:
            return np.array([])
        model = self._get_model()
        embeddings = model.encode(
            texts,
            normalize_embeddings=normalize,
            show_progress_bar=False,
        )
        return np.array(embeddings)

    def embed_single(self, text: str, normalize: bool = True) -> np.ndarray:
        """对单个文本进行 embedding"""
        return self.embed([text], normalize=normalize)[0]

    def get_dimension(self) -> int:
        """获取 embedding 维度"""
        model = self._get_model()
        # 获取模型输出维度（通过编码一个空文本）
        test_embedding = model.encode([""], normalize_embeddings=False)
        return test_embedding.shape[1]

    @staticmethod
    def compute_hash(text: str) -> str:
        """计算文本的 SHA256 哈希（用于去重）"""
        return hashlib.sha256(text.encode("utf-8")).hexdigest()


# 全局单例（懒加载）
_global_embedding_provider: Optional[EmbeddingProvider] = None


def get_embedding_provider(
    model_name: str = "BAAI/bge-m3",
    device: str = "cpu",
    cache_folder: Optional[str] = None,
) -> EmbeddingProvider:
    """获取全局 Embedding 提供者（单例模式）"""
    global _global_embedding_provider
    if _global_embedding_provider is None:
        _global_embedding_provider = EmbeddingProvider(
            model_name=model_name,
            device=device,
            cache_folder=cache_folder,
        )
    return _global_embedding_provider

