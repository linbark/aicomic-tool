"""
记忆索引器：将 SeriesBible/VisualDNA 等原子化切片并向量化索引
保留原 JSON 文件为真理源，向量库仅用于检索
"""
from __future__ import annotations

import json
from typing import Any, Dict, List, Optional

from .context_store import ContextStore
from .memory_store import MemoryStore, get_memory_store
from ..workflows.memory_schemas import MemoryNamespace, MemoryRecord, MemoryType


class MemoryIndexer:
    """
    记忆索引器
    负责将结构化数据（SeriesBible、VisualDNA）切片、向量化并写入记忆库
    """

    def __init__(self, memory_store: Optional[MemoryStore] = None, context_store: Optional[ContextStore] = None):
        """
        初始化索引器

        Args:
            memory_store: 记忆存储实例（None 则使用默认单例）
            context_store: 上下文存储实例（None 则使用默认单例）
        """
        self.memory_store = memory_store or get_memory_store()
        self.context_store = context_store or ContextStore()

    def index_series_bible(
        self,
        project_id: int,
        version: str = "v1",
        source_ref: Optional[str] = None,
    ) -> List[str]:
        """
        索引 SeriesBible（世界观设定）

        Args:
            project_id: 项目 ID
            version: 版本（默认 v1）
            source_ref: 来源引用

        Returns:
            创建的记录 ID 列表
        """
        bible = self.context_store.get_series_bible(project_id=project_id, version=version)
        if not bible:
            return []

        record_ids: List[str] = []

        # 1. 世界规则（world_rules）
        if isinstance(bible.get("world_rules"), dict):
            for key, value in bible["world_rules"].items():
                content = f"世界规则: {key}. {self._value_to_text(value)}"
                record = MemoryRecord(
                    project_id=project_id,
                    namespace=MemoryNamespace.STATIC_BIBLE,
                    type=MemoryType.WORLD_RULE,
                    entity=None,
                    content=content,
                    payload_json={"key": key, "value": value},
                    source_ref=source_ref or f"series_bible.{version}",
                )
                record_ids.append(self.memory_store.write(record))

        # 2. 角色设定（characters）
        characters = bible.get("characters")
        if isinstance(characters, dict):
            for char_name, char_data in characters.items():
                # 角色设计（Visual DNA 引用）
                if isinstance(char_data, dict):
                    visual_dna_ref = char_data.get("visual_dna_ref")
                    if visual_dna_ref:
                        content = f"角色 {char_name} 的视觉设计引用: {visual_dna_ref}"
                        record = MemoryRecord(
                            project_id=project_id,
                            namespace=MemoryNamespace.STATIC_BIBLE,
                            type=MemoryType.CHARACTER_DESIGN,
                            entity=char_name,
                            content=content,
                            payload_json={"character": char_name, "visual_dna_ref": visual_dna_ref},
                            source_ref=source_ref or f"series_bible.{version}",
                        )
                        record_ids.append(self.memory_store.write(record))

                    # 角色事实（背景、性格等）
                    char_facts = {k: v for k, v in char_data.items() if k != "visual_dna_ref"}
                    if char_facts:
                        content = f"角色 {char_name} 的设定: {self._value_to_text(char_facts)}"
                        record = MemoryRecord(
                            project_id=project_id,
                            namespace=MemoryNamespace.STATIC_BIBLE,
                            type=MemoryType.CHARACTER_FACT,
                            entity=char_name,
                            content=content,
                            payload_json={"character": char_name, "facts": char_facts},
                            source_ref=source_ref or f"series_bible.{version}",
                        )
                        record_ids.append(self.memory_store.write(record))

        # 3. 术语表（glossary）
        if isinstance(bible.get("glossary"), dict):
            for term, definition in bible["glossary"].items():
                content = f"术语 {term}: {self._value_to_text(definition)}"
                record = MemoryRecord(
                    project_id=project_id,
                    namespace=MemoryNamespace.STATIC_BIBLE,
                    type=MemoryType.WORLD_RULE,
                    entity=None,
                    content=content,
                    payload_json={"term": term, "definition": definition},
                    source_ref=source_ref or f"series_bible.{version}",
                )
                record_ids.append(self.memory_store.write(record))

        # 4. 约束/禁忌（constraints）
        constraints = bible.get("constraints")
        if isinstance(constraints, (list, dict)):
            if isinstance(constraints, list):
                for constraint in constraints:
                    content = f"约束: {self._value_to_text(constraint)}"
                    record = MemoryRecord(
                        project_id=project_id,
                        namespace=MemoryNamespace.WORLD_RULES_NEGATIVE,
                        type=MemoryType.NEGATIVE_CONSTRAINT,
                        entity=None,
                        content=content,
                        payload_json={"constraint": constraint},
                        source_ref=source_ref or f"series_bible.{version}",
                    )
                    record_ids.append(self.memory_store.write(record))
            elif isinstance(constraints, dict):
                for key, value in constraints.items():
                    content = f"约束 {key}: {self._value_to_text(value)}"
                    record = MemoryRecord(
                        project_id=project_id,
                        namespace=MemoryNamespace.WORLD_RULES_NEGATIVE,
                        type=MemoryType.NEGATIVE_CONSTRAINT,
                        entity=None,
                        content=content,
                        payload_json={"key": key, "value": value},
                        source_ref=source_ref or f"series_bible.{version}",
                    )
                    record_ids.append(self.memory_store.write(record))

        return record_ids

    def index_visual_dna(
        self,
        project_id: int,
        item_id: int,
        version: str = "v1",
        source_ref: Optional[str] = None,
    ) -> List[str]:
        """
        索引 Visual DNA（角色视觉设定）

        Args:
            project_id: 项目 ID
            item_id: 资产条目 ID
            version: 版本（默认 v1）
            source_ref: 来源引用

        Returns:
            创建的记录 ID 列表
        """
        visual_dna = self.context_store.get_visual_dna(project_id=project_id, item_id=item_id, version=version)
        if not visual_dna:
            return []

        record_ids: List[str] = []

        # 1. 角色核心（character_core）
        character_core = visual_dna.get("character_core")
        if isinstance(character_core, dict):
            # Visual DNA（不可变核心）
            visual_dna_data = character_core.get("visual_dna")
            if isinstance(visual_dna_data, dict):
                content_parts = []
                for key, value in visual_dna_data.items():
                    if value:
                        content_parts.append(f"{key}: {self._value_to_text(value)}")
                if content_parts:
                    content = f"角色视觉 DNA: {', '.join(content_parts)}"
                    record = MemoryRecord(
                        project_id=project_id,
                        namespace=MemoryNamespace.STATIC_BIBLE,
                        type=MemoryType.CHARACTER_DESIGN,
                        entity=f"item_{item_id}",
                        content=content,
                        payload_json={"item_id": item_id, "visual_dna": visual_dna_data},
                        source_ref=source_ref or f"visual_dna.asset_item_{item_id}.{version}",
                    )
                    record_ids.append(self.memory_store.write(record))

            # 服装（attire）
            attire = character_core.get("attire")
            if isinstance(attire, dict):
                content = f"角色服装: {self._value_to_text(attire)}"
                record = MemoryRecord(
                    project_id=project_id,
                    namespace=MemoryNamespace.STATIC_BIBLE,
                    type=MemoryType.CHARACTER_DESIGN,
                    entity=f"item_{item_id}",
                    content=content,
                    payload_json={"item_id": item_id, "attire": attire},
                    source_ref=source_ref or f"visual_dna.asset_item_{item_id}.{version}",
                )
                record_ids.append(self.memory_store.write(record))

        # 2. 技术参数（technical_specs）
        technical_specs = visual_dna.get("technical_specs")
        if isinstance(technical_specs, dict):
            content = f"技术参数: {self._value_to_text(technical_specs)}"
            record = MemoryRecord(
                project_id=project_id,
                namespace=MemoryNamespace.STATIC_BIBLE,
                type=MemoryType.STYLE_GUIDE,
                entity=f"item_{item_id}",
                content=content,
                payload_json={"item_id": item_id, "technical_specs": technical_specs},
                source_ref=source_ref or f"visual_dna.asset_item_{item_id}.{version}",
            )
            record_ids.append(self.memory_store.write(record))

        # 3. Stable Diffusion Tags（直接作为内容）
        sd_tags = visual_dna.get("stable_diffusion_tags")
        if isinstance(sd_tags, str) and sd_tags.strip():
            record = MemoryRecord(
                project_id=project_id,
                namespace=MemoryNamespace.STATIC_BIBLE,
                type=MemoryType.STYLE_GUIDE,
                entity=f"item_{item_id}",
                content=f"Stable Diffusion Tags: {sd_tags}",
                payload_json={"item_id": item_id, "stable_diffusion_tags": sd_tags},
                source_ref=source_ref or f"visual_dna.asset_item_{item_id}.{version}",
            )
            record_ids.append(self.memory_store.write(record))

        return record_ids

    def reindex_project(
        self,
        project_id: int,
        version: str = "v1",
    ) -> Dict[str, int]:
        """
        重新索引整个项目的记忆（SeriesBible + 所有 VisualDNA）

        Args:
            project_id: 项目 ID
            version: 版本

        Returns:
            统计信息（每个类型的记录数）
        """
        stats: Dict[str, int] = {}

        # 索引 SeriesBible
        bible_ids = self.index_series_bible(project_id=project_id, version=version)
        stats["series_bible"] = len(bible_ids)

        # 索引所有 VisualDNA（需要知道 item_id 列表，这里简化处理）
        # 实际使用时，可以从数据库或文件系统枚举所有 item_id
        # 这里先返回统计，实际实现需要根据项目结构调整

        return stats

    @staticmethod
    def _value_to_text(value: Any) -> str:
        """将任意值转换为文本（用于 embedding）"""
        if isinstance(value, str):
            return value
        elif isinstance(value, (dict, list)):
            return json.dumps(value, ensure_ascii=False)
        else:
            return str(value)

