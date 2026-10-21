#!/usr/bin/env python3
"""
清空数据库中所有表的数据，但保留表结构
"""
import argparse
import os
import sqlite3
from sqlalchemy import inspect, text

from ..database import engine, Base
from ..models import (
    Project, Character, Episode, Scene, Shot, Asset, Event, EventNode, AiActionRun
)
from ..services.app_paths import app_data_dir


def clear_main_database():
    """清空主数据库（database.db）的所有数据"""
    print("=" * 80)
    print("清空主数据库 (database.db)")
    print("=" * 80)
    
    inspector = inspect(engine)
    tables = inspector.get_table_names()
    
    if not tables:
        print("数据库中没有表，无需清空")
        return
    
    # 获取所有表名（按依赖关系排序，先删除子表，再删除父表）
    # SQLite 中，我们需要考虑外键关系
    # 按照依赖关系排序：先删除有外键的表，再删除被引用的表
    table_order = [
        "event_nodes",      # 依赖 events
        "shots",            # 依赖 scenes
        "scenes",           # 依赖 episodes
        "assets",           # 依赖 characters 和 shots
        "characters",       # 依赖 projects
        "episodes",         # 依赖 projects
        "events",           # 依赖 projects
        "ai_action_runs",   # 依赖 projects
        "projects",         # 根表
    ]
    
    # 只保留实际存在的表
    tables_to_clear = [t for t in table_order if t in tables]
    # 添加其他未在列表中的表
    for t in tables:
        if t not in tables_to_clear:
            tables_to_clear.append(t)
    
    print(f"\n找到 {len(tables_to_clear)} 个表，准备清空数据...")
    
    with engine.begin() as conn:
        # 禁用外键约束检查
        conn.execute(text("PRAGMA foreign_keys = OFF"))
        
        deleted_counts = {}
        for table_name in tables_to_clear:
            try:
                # 获取删除前的记录数
                result = conn.execute(text(f"SELECT COUNT(*) FROM {table_name}"))
                count_before = result.scalar()
                
                # 删除所有数据
                result = conn.execute(text(f"DELETE FROM {table_name}"))
                deleted_count = result.rowcount
                
                deleted_counts[table_name] = {
                    "before": count_before,
                    "deleted": deleted_count
                }
                print(f"  ✓ {table_name}: 删除了 {deleted_count} 条记录（原有 {count_before} 条）")
            except Exception as e:
                print(f"  ✗ {table_name}: 清空失败 - {e}")
                deleted_counts[table_name] = {"error": str(e)}
        
        # 重新启用外键约束检查
        conn.execute(text("PRAGMA foreign_keys = ON"))
    
    print(f"\n主数据库清空完成！")
    print(f"总计清空了 {len([t for t in deleted_counts if 'error' not in deleted_counts[t]])} 个表")
    return deleted_counts


def clear_memory_database():
    """清空记忆数据库（memory_store.db）的所有数据"""
    print("\n\n" + "=" * 80)
    print("清空记忆数据库 (memory_store.db)")
    print("=" * 80)
    
    db_path = os.path.join(app_data_dir(), "memory_store.db")
    
    if not os.path.exists(db_path):
        print(f"数据库文件不存在: {db_path}")
        return {}
    
    conn = sqlite3.connect(db_path)
    cursor = conn.cursor()
    
    # 获取所有表名
    cursor.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
    tables = [row[0] for row in cursor.fetchall()]
    
    if not tables:
        print("数据库中没有表，无需清空")
        conn.close()
        return {}
    
    print(f"\n找到 {len(tables)} 个表，准备清空数据...")
    
    # 按照依赖关系排序（根据外键关系）
    # canonical 表的依赖关系比较复杂，我们按照创建顺序的反序删除
    table_order = [
        "canonical_conflicts",        # 依赖 changesets
        "canonical_changesets",       # 依赖其他 canonical 表
        "canonical_state_changes",    # 依赖 events, snapshots
        "canonical_snapshots",        # 依赖 entities
        "canonical_time_blocks",      # 依赖 events
        "canonical_time_constraints", # 依赖 events
        "canonical_events",           # 依赖 evidences
        "canonical_entities",         # 依赖 evidences
        "canonical_evidences",        # 根表
        "episodic_memories",          # 依赖 memory_records
        "memory_records",             # 根表
    ]
    
    # 只保留实际存在的表
    tables_to_clear = [t for t in table_order if t in tables]
    # 添加其他未在列表中的表
    for t in tables:
        if t not in tables_to_clear:
            tables_to_clear.append(t)
    
    # 禁用外键约束检查
    cursor.execute("PRAGMA foreign_keys = OFF")
    
    deleted_counts = {}
    for table_name in tables_to_clear:
        try:
            # 获取删除前的记录数
            cursor.execute(f"SELECT COUNT(*) FROM {table_name}")
            count_before = cursor.fetchone()[0]
            
            # 删除所有数据
            cursor.execute(f"DELETE FROM {table_name}")
            deleted_count = cursor.rowcount
            
            deleted_counts[table_name] = {
                "before": count_before,
                "deleted": deleted_count
            }
            print(f"  ✓ {table_name}: 删除了 {deleted_count} 条记录（原有 {count_before} 条）")
        except Exception as e:
            print(f"  ✗ {table_name}: 清空失败 - {e}")
            deleted_counts[table_name] = {"error": str(e)}
    
    # 重新启用外键约束检查
    cursor.execute("PRAGMA foreign_keys = ON")
    
    conn.commit()
    conn.close()
    
    print(f"\n记忆数据库清空完成！")
    print(f"总计清空了 {len([t for t in deleted_counts if 'error' not in deleted_counts[t]])} 个表")
    return deleted_counts


def main():
    """主函数"""
    parser = argparse.ArgumentParser(description="清空数据库中所有表的数据，但保留表结构")
    parser.add_argument(
        "--yes",
        action="store_true",
        help="跳过确认提示，直接执行清空操作"
    )
    parser.add_argument(
        "--main-only",
        action="store_true",
        help="只清空主数据库（database.db）"
    )
    parser.add_argument(
        "--memory-only",
        action="store_true",
        help="只清空记忆数据库（memory_store.db）"
    )
    args = parser.parse_args()
    
    print("\n数据库清空工具")
    print("=" * 80)
    print("警告：此操作将删除所有数据库中的数据，但保留表结构！")
    print("=" * 80)
    
    # 确认操作（除非使用了 --yes 参数）
    if not args.yes:
        response = input("\n确认要清空所有数据库吗？(输入 'yes' 确认): ")
        if response.lower() != 'yes':
            print("操作已取消")
            return
    
    print("\n开始清空数据库...\n")
    
    main_counts = {}
    memory_counts = {}
    
    # 根据参数决定清空哪些数据库
    if args.memory_only:
        memory_counts = clear_memory_database()
    elif args.main_only:
        main_counts = clear_main_database()
    else:
        # 清空主数据库
        main_counts = clear_main_database()
        # 清空记忆数据库
        memory_counts = clear_memory_database()
    
    # 总结
    print("\n" + "=" * 80)
    print("清空操作完成")
    print("=" * 80)
    if main_counts:
        print(f"\n主数据库: 清空了 {len([t for t in main_counts if 'error' not in main_counts.get(t, {})])} 个表")
    if memory_counts:
        print(f"记忆数据库: 清空了 {len([t for t in memory_counts if 'error' not in memory_counts.get(t, {})])} 个表")
    print("\n所有表结构已保留，可以重新开始使用数据库。")


if __name__ == "__main__":
    main()
