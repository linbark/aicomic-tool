"""
Evidence Ingestor（原文证据切片）

目标：
- 将“章节原文”切片为一组 EvidenceRecordPayload
- 保持 Evidence-first：每条记忆/推断都能回溯到 evidence_id

当前策略（最小可用）：
- 先按空行分段（paragraph）
- 每段过长则按字符长度继续切分
"""

from __future__ import annotations

from typing import List, Optional

from ..workflows.memory_schemas import EvidenceRecordPayload, EvidenceSpan

from ..routers.ai_helpers import log_ui
import logging

logger = logging.getLogger(__name__)

def _split_by_blank_lines(text: str) -> List[str]:
    lines = (text or "").splitlines()
    paras: List[str] = []
    buf: List[str] = []
    for ln in lines:
        if not (ln or "").strip():
            if buf:
                paras.append("\n".join(buf).strip())
                buf = []
            continue
        buf.append(ln.rstrip())
    if buf:
        paras.append("\n".join(buf).strip())
    return [p for p in paras if p]


def _chunk_by_chars(s: str, max_chars: int) -> List[str]:
    s = (s or "").strip()
    if not s:
        return []
    if max_chars <= 0:
        return [s]
    if len(s) <= max_chars:
        return [s]
    out: List[str] = []
    i = 0
    while i < len(s):
        out.append(s[i : i + max_chars].strip())
        i += max_chars
    return [x for x in out if x]


def chunk_text_to_evidences(
    *,
    project_id: int,
    run_id: str,
    text: str,
    episode_id: Optional[int] = None,
    scene_id: Optional[int] = None,
    max_quote_chars: int = 600,
    tags: Optional[List[str]] = None,
) -> List[EvidenceRecordPayload]:
    """
    将一段原文切片为 evidence 列表。

    - paragraph_index：按空行分段后的段落序号
    - sentence_index：暂不解析（保留接口以便后续升级为句级切片）
    - start/end_offset：暂不计算（保留接口以便后续升级为字符级定位）
    """
    tags = tags or []
    
    logger.info(f"Chunking text for project {project_id}, length: {len(text)}")
    paras = _split_by_blank_lines(text)
    evidences: List[EvidenceRecordPayload] = []

    for p_idx, para in enumerate(paras):
        para_preview = (para or "").strip()
        if len(para_preview) > 200:
            para_preview = para_preview[:200] + "..."
        log_ui(
            project_id,
            run_id,
            {
                "stage": "memory.evidence.chunk.paragraph",
                "summary": "切片段落",
                "data": {"paragraph_index": int(p_idx), "para_len": len(para or ""), "para_preview": para_preview},
            },
            "INFO",
        )
        chunks = _chunk_by_chars(para, max_quote_chars)
        logger.debug(f"Paragraph {p_idx} split into {len(chunks)} chunks")
        for c_idx, chunk in enumerate(chunks):
            chunk_preview = (chunk or "").strip()
            if len(chunk_preview) > 200:
                chunk_preview = chunk_preview[:200] + "..."
            log_ui(
                project_id,
                run_id,
                {
                    "stage": "memory.evidence.chunk.chunk",
                    "summary": "切片 chunk",
                    "data": {
                        "paragraph_index": int(p_idx),
                        "chunk_index": int(c_idx),
                        "chunk_total": int(len(chunks)),
                        "chunk_len": len(chunk or ""),
                        "chunk_preview": chunk_preview,
                    },
                },
                "INFO",
            )
            span = EvidenceSpan(paragraph_index=int(p_idx), sentence_index=None, start_offset=None, end_offset=None)
            log_ui(
                project_id,
                run_id,
                {
                    "stage": "memory.evidence.chunk.span",
                    "summary": "生成 span",
                    "data": {"paragraph_index": int(p_idx), "chunk_index": int(c_idx), "span": span.model_dump()},
                },
                "INFO",
            )
            # 如果同段落被切成多块，用 tags 标注 chunk 序号（便于回溯）
            extra_tags = list(tags)
            if len(chunks) > 1:
                extra_tags.append(f"chunk:{c_idx+1}/{len(chunks)}")
            evidences.append(
                EvidenceRecordPayload(
                    project_id=int(project_id),
                    episode_id=episode_id,
                    scene_id=scene_id,
                    span=span,
                    quote=str(chunk),
                    speaker=None,
                    tags=extra_tags,
                )
            )

    logger.info(f"Chunking finished. Total evidences generated: {len(evidences)}")
    return evidences
