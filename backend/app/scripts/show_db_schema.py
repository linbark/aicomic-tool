#!/usr/bin/env python3
"""
查看数据库中所有表和字段的脚本
"""
import os
import sqlite3
from pathlib import Path
from typing import List, Tuple

from sqlalchemy import inspect, create_engine, text

from ..database import SQLALCHEMY_DATABASE_URL, engine, Base
from ..models import (
    Project, Character, Episode, Scene, Shot, Asset, Event, EventNode, AiActionRun
)
from ..services.app_paths import app_data_dir


def get_table_info_sqlite(conn: sqlite3.Connection, table_name: str) -> List[Tuple]:
    """获取SQLite表的字段信息"""
    cursor = conn.cursor()
    cursor.execute(f"PRAGMA table_info({table_name})")
    return cursor.fetchall()


def show_main_database():
    """显示主数据库（database.db）的表和字段"""
    print("=" * 80)
    print("主数据库 (database.db)")
    print("=" * 80)
    
    inspector = inspect(engine)
    tables = inspector.get_table_names()
    
    # 如果数据库为空，从模型定义中获取表结构
    if not tables:
        print("\n注意: 数据库为空，从模型定义中显示表结构\n")
        # 获取所有模型类
        model_classes = {
            Project.__tablename__: Project,
            Character.__tablename__: Character,
            Episode.__tablename__: Episode,
            Scene.__tablename__: Scene,
            Shot.__tablename__: Shot,
            Asset.__tablename__: Asset,
            Event.__tablename__: Event,
            EventNode.__tablename__: EventNode,
            AiActionRun.__tablename__: AiActionRun,
        }
        
        for table_name in sorted(model_classes.keys()):
            model_class = model_classes[table_name]
            print(f"\n表名: {table_name}")
            print("-" * 80)
            print(f"{'字段名':<30} {'类型':<20} {'可空':<8} {'主键':<8} {'外键':<30}")
            print("-" * 80)
            
            for col in model_class.__table__.columns:
                is_pk = "✓" if col.primary_key else ""
                nullable = "是" if col.nullable else "否"
                col_type = str(col.type)
                
                # 检查是否是外键
                fk_info = ""
                for fk in col.foreign_keys:
                    fk_info = f"{fk.column.table.name}.{fk.column.name}"
                    break
                
                print(f"{col.name:<30} {col_type:<20} {nullable:<8} {is_pk:<8} {fk_info:<30}")
            
            # 显示外键关系
            fks = []
            for col in model_class.__table__.columns:
                for fk in col.foreign_keys:
                    fks.append((col.name, fk.column.table.name, fk.column.name))
            
            if fks:
                print("\n外键关系:")
                for col_name, ref_table, ref_col in fks:
                    print(f"  {col_name} -> {ref_table}.{ref_col}")
    else:
        # 数据库有数据，从实际数据库中读取
        for table_name in sorted(tables):
            print(f"\n表名: {table_name}")
            print("-" * 80)
            
            columns = inspector.get_columns(table_name)
            print(f"{'字段名':<30} {'类型':<20} {'可空':<8} {'主键':<8} {'默认值':<15}")
            print("-" * 80)
            
            # 获取主键信息
            pk_constraint = inspector.get_pk_constraint(table_name)
            pk_columns = set(pk_constraint.get('constrained_columns', []))
            
            for col in columns:
                is_pk = "✓" if col['name'] in pk_columns else ""
                nullable = "是" if col['nullable'] else "否"
                default = str(col.get('default', '')) if col.get('default') is not None else ""
                col_type = str(col['type'])
                print(f"{col['name']:<30} {col_type:<20} {nullable:<8} {is_pk:<8} {default:<15}")
            
            # 显示外键信息
            fks = inspector.get_foreign_keys(table_name)
            if fks:
                print("\n外键关系:")
                for fk in fks:
                    print(f"  {fk['constrained_columns']} -> {fk['referred_table']}.{fk['referred_columns']}")


def parse_create_table_sql(sql: str) -> dict:
    """解析CREATE TABLE SQL语句，返回表结构信息"""
    import re
    
    # 提取表名
    table_match = re.search(r'CREATE TABLE (?:IF NOT EXISTS )?(\w+)', sql, re.IGNORECASE)
    if not table_match:
        return None
    
    table_name = table_match.group(1)
    
    # 提取字段定义
    fields = []
    # 匹配字段定义: name type [constraints]
    field_pattern = r'(\w+)\s+(\w+(?:\([^)]+\))?)\s*([^,)]*)'
    
    # 提取括号内的内容
    content_match = re.search(r'\((.*)\)', sql, re.DOTALL)
    if not content_match:
        return None
    
    content = content_match.group(1)
    
    # 分割字段定义（考虑括号嵌套）
    lines = []
    current = ""
    paren_depth = 0
    
    for char in content:
        if char == '(':
            paren_depth += 1
            current += char
        elif char == ')':
            paren_depth -= 1
            current += char
        elif char == ',' and paren_depth == 0:
            if current.strip():
                lines.append(current.strip())
            current = ""
        else:
            current += char
    
    if current.strip():
        lines.append(current.strip())
    
    # 解析每个字段
    pk_fields = set()
    fk_fields = []
    
    for line in lines:
        line = line.strip()
        if not line or line.upper().startswith('FOREIGN KEY') or line.upper().startswith('PRIMARY KEY'):
            # 处理复合主键或外键
            if 'PRIMARY KEY' in line.upper():
                pk_match = re.search(r'PRIMARY KEY\s*\(([^)]+)\)', line, re.IGNORECASE)
                if pk_match:
                    pk_fields.update([f.strip() for f in pk_match.group(1).split(',')])
            elif 'FOREIGN KEY' in line.upper():
                fk_match = re.search(r'FOREIGN KEY\s*\(([^)]+)\)\s*REFERENCES\s+(\w+)\.(\w+)', line, re.IGNORECASE)
                if fk_match:
                    fk_fields.append((fk_match.group(1).strip(), fk_match.group(2), fk_match.group(3)))
            continue
        
        # 解析字段定义
        parts = line.split()
        if len(parts) < 2:
            continue
        
        field_name = parts[0]
        field_type = parts[1]
        
        # 检查是否是主键
        is_pk = 'PRIMARY KEY' in line.upper()
        if is_pk:
            pk_fields.add(field_name)
        
        # 检查是否可空
        nullable = 'NOT NULL' not in line.upper()
        
        # 检查外键
        fk_ref = None
        if 'REFERENCES' in line.upper():
            ref_match = re.search(r'REFERENCES\s+(\w+)\.(\w+)', line, re.IGNORECASE)
            if ref_match:
                fk_ref = f"{ref_match.group(1)}.{ref_match.group(2)}"
        
        fields.append({
            'name': field_name,
            'type': field_type,
            'nullable': nullable,
            'pk': is_pk,
            'fk': fk_ref
        })
    
    return {
        'name': table_name,
        'fields': fields,
        'pk_fields': pk_fields,
        'fk_fields': fk_fields
    }


def show_memory_database():
    """显示记忆数据库（memory_store.db）的表和字段"""
    print("\n\n" + "=" * 80)
    print("记忆数据库 (memory_store.db)")
    print("=" * 80)
    
    db_path = os.path.join(app_data_dir(), "memory_store.db")
    
    if not os.path.exists(db_path):
        print(f"\n注意: 数据库文件不存在 ({db_path})，从代码定义中显示表结构\n")
        
        # 从memory_store.py中定义的CREATE TABLE语句解析表结构
        table_definitions = [
            {
                'name': 'memory_records',
                'sql': """CREATE TABLE memory_records (
                    id TEXT PRIMARY KEY,
                    project_id INTEGER NOT NULL,
                    namespace TEXT NOT NULL,
                    type TEXT NOT NULL,
                    entity TEXT,
                    content TEXT NOT NULL,
                    payload_json TEXT,
                    source_ref TEXT,
                    time_index TEXT,
                    status TEXT,
                    confidence REAL,
                    source_kind TEXT,
                    evidence_ids_json TEXT,
                    story_order TEXT,
                    story_time_json TEXT,
                    hash TEXT,
                    created_at_ms INTEGER,
                    updated_at_ms INTEGER
                )"""
            },
            {
                'name': 'episodic_memories',
                'sql': """CREATE TABLE episodic_memories (
                    id TEXT PRIMARY KEY,
                    project_id INTEGER NOT NULL,
                    event TEXT NOT NULL,
                    state_changes_json TEXT NOT NULL,
                    entities_json TEXT NOT NULL,
                    episode_id INTEGER,
                    scene_id INTEGER,
                    beat_index INTEGER,
                    created_at_ms INTEGER,
                    FOREIGN KEY (id) REFERENCES memory_records(id)
                )"""
            },
            {
                'name': 'canonical_evidences',
                'sql': """CREATE TABLE canonical_evidences (
                    evidence_id TEXT PRIMARY KEY,
                    project_id INTEGER NOT NULL,
                    episode_id INTEGER,
                    scene_id INTEGER,
                    span_json TEXT,
                    quote TEXT NOT NULL,
                    speaker TEXT,
                    tags_json TEXT,
                    created_at_ms INTEGER
                )"""
            },
            {
                'name': 'canonical_entities',
                'sql': """CREATE TABLE canonical_entities (
                    entity_id TEXT PRIMARY KEY,
                    project_id INTEGER NOT NULL,
                    entity_type TEXT NOT NULL,
                    canonical_name TEXT NOT NULL,
                    aliases_json TEXT,
                    status TEXT,
                    confidence REAL,
                    source_kind TEXT,
                    created_from_evidence_id TEXT,
                    created_at_ms INTEGER,
                    updated_at_ms INTEGER
                )"""
            },
            {
                'name': 'canonical_events',
                'sql': """CREATE TABLE canonical_events (
                    event_id TEXT PRIMARY KEY,
                    project_id INTEGER NOT NULL,
                    story_order TEXT NOT NULL,
                    story_time_key TEXT,
                    story_time_json TEXT,
                    episode_id INTEGER,
                    scene_id INTEGER,
                    event_type TEXT,
                    summary TEXT NOT NULL,
                    participants_json TEXT,
                    evidence_ids_json TEXT,
                    status TEXT,
                    confidence REAL,
                    source_kind TEXT,
                    created_at_ms INTEGER,
                    updated_at_ms INTEGER
                )"""
            },
            {
                'name': 'canonical_time_constraints',
                'sql': """CREATE TABLE canonical_time_constraints (
                    constraint_id TEXT PRIMARY KEY,
                    project_id INTEGER NOT NULL,
                    relation TEXT NOT NULL,
                    from_event_id TEXT,
                    to_event_id TEXT,
                    anchor_id TEXT,
                    interval_json TEXT,
                    evidence_ids_json TEXT,
                    status TEXT,
                    confidence REAL,
                    source_kind TEXT,
                    created_at_ms INTEGER,
                    updated_at_ms INTEGER
                )"""
            },
            {
                'name': 'canonical_time_blocks',
                'sql': """CREATE TABLE canonical_time_blocks (
                    block_id TEXT PRIMARY KEY,
                    project_id INTEGER NOT NULL,
                    name TEXT,
                    parent_block_id TEXT,
                    anchor_id TEXT,
                    constraints_json TEXT,
                    event_ids_json TEXT,
                    status TEXT,
                    confidence REAL,
                    source_kind TEXT,
                    created_at_ms INTEGER,
                    updated_at_ms INTEGER
                )"""
            },
            {
                'name': 'canonical_snapshots',
                'sql': """CREATE TABLE canonical_snapshots (
                    snapshot_id TEXT PRIMARY KEY,
                    project_id INTEGER NOT NULL,
                    entity_id TEXT NOT NULL,
                    valid_from_story_time_key TEXT,
                    valid_to_story_time_key TEXT,
                    valid_from_story_order TEXT,
                    valid_to_story_order TEXT,
                    fields_json TEXT NOT NULL,
                    why TEXT,
                    evidence_ids_json TEXT,
                    status TEXT,
                    confidence REAL,
                    source_kind TEXT,
                    created_at_ms INTEGER,
                    updated_at_ms INTEGER
                )"""
            },
            {
                'name': 'canonical_state_changes',
                'sql': """CREATE TABLE canonical_state_changes (
                    state_change_id TEXT PRIMARY KEY,
                    project_id INTEGER NOT NULL,
                    event_id TEXT NOT NULL,
                    target_entity_id TEXT,
                    patch_json TEXT NOT NULL,
                    before_snapshot_id TEXT,
                    after_snapshot_id TEXT,
                    evidence_ids_json TEXT,
                    status TEXT,
                    confidence REAL,
                    source_kind TEXT,
                    created_at_ms INTEGER,
                    updated_at_ms INTEGER
                )"""
            },
            {
                'name': 'canonical_changesets',
                'sql': """CREATE TABLE canonical_changesets (
                    changeset_id TEXT PRIMARY KEY,
                    project_id INTEGER NOT NULL,
                    episode_id INTEGER,
                    payload_json TEXT NOT NULL,
                    review_status TEXT NOT NULL,
                    review_log_json TEXT,
                    created_at_ms INTEGER,
                    updated_at_ms INTEGER
                )"""
            },
            {
                'name': 'canonical_conflicts',
                'sql': """CREATE TABLE canonical_conflicts (
                    conflict_id TEXT PRIMARY KEY,
                    project_id INTEGER NOT NULL,
                    changeset_id TEXT,
                    conflict_type TEXT NOT NULL,
                    entity_id TEXT,
                    old_claim_json TEXT,
                    new_claim_json TEXT,
                    suggested_actions_json TEXT,
                    status TEXT NOT NULL,
                    resolved_by TEXT,
                    resolution_note TEXT,
                    created_at_ms INTEGER,
                    updated_at_ms INTEGER
                )"""
            },
        ]
        
        for table_def in table_definitions:
            table_info = parse_create_table_sql(table_def['sql'])
            if not table_info:
                continue
            
            print(f"\n表名: {table_info['name']}")
            print("-" * 80)
            print(f"{'字段名':<30} {'类型':<20} {'可空':<8} {'主键':<8} {'外键':<30}")
            print("-" * 80)
            
            for field in table_info['fields']:
                is_pk = "✓" if field['name'] in table_info['pk_fields'] else ""
                nullable = "是" if field['nullable'] else "否"
                fk_info = field.get('fk', '') or ""
                print(f"{field['name']:<30} {field['type']:<20} {nullable:<8} {is_pk:<8} {fk_info:<30}")
            
            if table_info['fk_fields']:
                print("\n外键关系:")
                for fk_col, ref_table, ref_col in table_info['fk_fields']:
                    print(f"  {fk_col} -> {ref_table}.{ref_col}")
        
        return
    
    # 数据库文件存在，从实际数据库中读取
    conn = sqlite3.connect(db_path)
    cursor = conn.cursor()
    
    # 获取所有表名
    cursor.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
    tables = [row[0] for row in cursor.fetchall()]
    
    if not tables:
        print("数据库中没有表")
        conn.close()
        return
    
    for table_name in tables:
        print(f"\n表名: {table_name}")
        print("-" * 80)
        
        # 获取表结构信息
        table_info = get_table_info_sqlite(conn, table_name)
        
        print(f"{'字段名':<30} {'类型':<20} {'可空':<8} {'主键':<8} {'默认值':<15}")
        print("-" * 80)
        
        for row in table_info:
            cid, name, col_type, notnull, default_val, pk = row
            is_pk = "✓" if pk else ""
            nullable = "否" if notnull else "是"
            default = str(default_val) if default_val is not None else ""
            print(f"{name:<30} {col_type:<20} {nullable:<8} {is_pk:<8} {default:<15}")
        
        # 获取索引信息
        cursor.execute(f"SELECT name, sql FROM sqlite_master WHERE type='index' AND tbl_name='{table_name}'")
        indexes = cursor.fetchall()
        if indexes:
            print("\n索引:")
            for idx_name, idx_sql in indexes:
                if idx_name.startswith("sqlite_autoindex"):
                    continue
                print(f"  {idx_name}: {idx_sql}")
    
    conn.close()


def main():
    """主函数"""
    print("\n数据库表结构查看工具\n")
    
    # 显示主数据库
    show_main_database()
    
    # 显示记忆数据库
    show_memory_database()
    
    print("\n" + "=" * 80)
    print("完成")
    print("=" * 80)


if __name__ == "__main__":
    main()
