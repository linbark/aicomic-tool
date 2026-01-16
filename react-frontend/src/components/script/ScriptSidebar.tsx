import { memo, useState } from 'react'
import type { EpisodeRead } from '../../api/types'
import type { Selected, ScriptStep } from './types'
import { panelStyle } from '../../styles/shared'

type ScriptSidebarProps = {
  episodes: EpisodeRead[]
  selected: Selected
  onSelect: (episodeId: number, step: ScriptStep) => void
  onDeleteEpisode: (episodeId: number) => void
}

export const ScriptSidebar = memo(function ScriptSidebar({
  episodes,
  selected,
  onSelect,
  onDeleteEpisode,
}: ScriptSidebarProps) {
  const [expanded, setExpanded] = useState<Record<number, boolean>>({})

  return (
    <div style={panelStyle}>
      <div style={styles.sidebarSection}>
        <div style={styles.colHeader}>
          <div style={styles.colTitle}>剧本目录</div>
        </div>
        <div style={styles.colScroll} className="aicomic-scroll">
          {episodes.map((ep) => {
            const isExpanded =
              selected.kind === 'episode' && selected.episodeId === ep.id ? expanded[ep.id] !== false : !!expanded[ep.id]
            const isSelected = selected.kind === 'episode' && selected.episodeId === ep.id
            
            return (
              <div key={ep.id} style={styles.group}>
                <div style={styles.groupHeader}>
                  <button 
                    style={styles.toggleBtn}
                    onClick={() => setExpanded((prev) => ({ ...prev, [ep.id]: !isExpanded }))}
                  >
                    {isExpanded ? '▼' : '▶'}
                  </button>
                  <div style={styles.groupTitle} onClick={() => setExpanded((prev) => ({ ...prev, [ep.id]: !isExpanded }))}>
                    <span style={styles.mono}>EP{ep.order}</span> {ep.title}
                  </div>
                  <button 
                    style={styles.deleteBtn}
                    onClick={(e) => {
                      e.stopPropagation()
                      onDeleteEpisode(ep.id)
                    }}
                    title="删除本集"
                  >
                    ×
                  </button>
                </div>
                
                {isExpanded && (
                  <div style={styles.stepList}>
                    <StepItem 
                      label="Step 0: 原始剧本" 
                      active={isSelected && selected.step === 0}
                      onClick={() => onSelect(ep.id, 0)}
                    />
                    <StepItem 
                      label="Step 1: 结构化拆解" 
                      active={isSelected && selected.step === 1}
                      onClick={() => onSelect(ep.id, 1)}
                    />
                    <StepItem 
                      label="Step 2: 资产提取" 
                      active={isSelected && selected.step === 2}
                      onClick={() => onSelect(ep.id, 2)}
                    />
                  </div>
                )}
              </div>
            )
          })}
        </div>
      </div>
    </div>
  )
})

function StepItem({ label, active, onClick }: { label: string, active: boolean, onClick: () => void }) {
  return (
    <button
      style={{
        ...styles.stepBtn,
        ...(active ? styles.stepBtnActive : null)
      }}
      onClick={onClick}
    >
      {label}
    </button>
  )
}

const styles: Record<string, React.CSSProperties> = {
  sidebarSection: {
    display: 'flex',
    flexDirection: 'column',
    height: 'calc(100vh - 110px)',
  },
  colHeader: {
    padding: 12,
    borderBottom: '1px solid rgba(255,255,255,0.08)',
    display: 'flex',
    justifyContent: 'space-between',
    alignItems: 'center',
  },
  colTitle: { fontWeight: 700, fontSize: 13 },
  colScroll: { padding: 10, overflowY: 'auto', flex: 1 },
  group: {
    marginBottom: 4,
  },
  groupHeader: {
    display: 'flex',
    alignItems: 'center',
    gap: 8,
    padding: '6px 8px',
    borderRadius: 8,
    cursor: 'pointer',
    background: 'rgba(255,255,255,0.04)',
    border: '1px solid rgba(255,255,255,0.06)',
    marginBottom: 4,
  },
  deleteBtn: {
    background: 'transparent',
    border: 'none',
    color: 'rgba(255,255,255,0.3)',
    cursor: 'pointer',
    fontSize: 14,
    width: 20,
    display: 'flex', 
    justifyContent: 'center',
    marginLeft: 4,
  },
  toggleBtn: {
    background: 'transparent',
    border: 'none',
    color: 'rgba(255,255,255,0.5)',
    cursor: 'pointer',
    fontSize: 10,
    width: 20, 
    display: 'flex', justifyContent: 'center'
  },
  groupTitle: {
    fontSize: 12, fontWeight: 700, flex: 1,
    whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis'
  },
  mono: { fontFamily: 'monospace', fontSize: 11, opacity: 0.7, marginRight: 6 },
  stepList: {
    display: 'flex',
    flexDirection: 'column',
    gap: 2,
    paddingLeft: 20,
    marginBottom: 8,
  },
  stepBtn: {
    textAlign: 'left',
    background: 'transparent',
    border: '1px solid transparent',
    color: 'rgba(255,255,255,0.65)',
    borderRadius: 6,
    padding: '6px 10px',
    cursor: 'pointer',
    fontSize: 12,
    transition: 'all 150ms ease',
  },
  stepBtnActive: {
    background: 'rgba(99,102,241,0.15)',
    border: '1px solid rgba(99,102,241,0.4)',
    color: '#fff',
    fontWeight: 500,
  }
}
