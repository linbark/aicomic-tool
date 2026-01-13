import { memo } from 'react'
import type { EpisodeRead } from '../../api/types'
import type { Selected } from './types'
import { trim } from './utils'
import { panelStyle } from '../../styles/shared'

type EpisodeListProps = {
  episodes: EpisodeRead[]
  selected: Selected
  selectedEpisodeId: number | null
  selectedSceneId: number | null
  onSelectEpisode: (episodeId: number) => void
  onSelectScene: (episodeId: number, sceneId: number) => void
}

export const EpisodeList = memo(function EpisodeList({
  episodes,
  selected,
  selectedEpisodeId,
  selectedSceneId,
  onSelectEpisode,
  onSelectScene,
}: EpisodeListProps) {
  return (
    <div style={panelStyle}>
      <div style={styles.sidebarSection}>
        <div style={styles.colHeader}>
          <div style={styles.colTitle}>Episodes</div>
        </div>
        <div style={styles.colScroll} className="aicomic-scroll">
          {episodes.map((ep) => (
            <div key={ep.id} style={styles.card}>
              <button
                style={{
                  ...styles.cardBtn,
                  ...(selected.kind !== 'none' && selectedEpisodeId === ep.id ? styles.cardActive : null),
                }}
                onClick={() => onSelectEpisode(ep.id)}
                onFocus={(e) => {
                  e.currentTarget.style.outline = '2px solid rgba(99,102,241,0.6)'
                  e.currentTarget.style.outlineOffset = '2px'
                }}
                onBlur={(e) => {
                  e.currentTarget.style.outline = 'none'
                }}
              >
                <div style={styles.cardTitle}>
                  <span style={styles.mono}>EP{ep.order}</span> {ep.title}
                </div>
                {ep.description ? <div style={styles.cardSub}>{trim(ep.description, 80)}</div> : null}
              </button>

              {(ep.scenes || []).length ? (
                <div style={{ marginTop: 8, display: 'flex', flexDirection: 'column', gap: 6, paddingLeft: 10 }}>
                  {(ep.scenes || []).map((sc) => (
                    <button
                      key={sc.id}
                      style={{
                        ...styles.smallListBtn,
                        ...(selected.kind !== 'none' && selectedSceneId === sc.id ? styles.smallListBtnActive : null),
                      }}
                      onClick={() => onSelectScene(ep.id, sc.id)}
                      onFocus={(e) => {
                        e.currentTarget.style.outline = '2px solid rgba(99,102,241,0.6)'
                        e.currentTarget.style.outlineOffset = '2px'
                      }}
                      onBlur={(e) => {
                        e.currentTarget.style.outline = 'none'
                      }}
                    >
                      <span style={styles.mono}>SC{sc.sequence_number ?? ''}</span> {sc.title || '未命名场景'}
                    </button>
                  ))}
                </div>
              ) : null}
            </div>
          ))}
        </div>
      </div>
    </div>
  )
})

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
  card: {
    marginBottom: 10,
  },
  cardBtn: {
    width: '100%',
    textAlign: 'left',
    background: 'rgba(0,0,0,0.18)',
    border: '1px solid rgba(255,255,255,0.08)',
    color: 'rgba(255,255,255,0.92)',
    borderRadius: 10,
    padding: 10,
    cursor: 'pointer',
    transition: 'all 200ms ease',
  },
  cardActive: {
    border: '1px solid rgba(99,102,241,0.7)',
    background: 'rgba(99,102,241,0.12)',
  },
  cardTitle: { fontWeight: 700, fontSize: 12, marginBottom: 4 },
  cardSub: { fontSize: 11, opacity: 0.7, whiteSpace: 'pre-wrap' as const },
  smallListBtn: {
    width: '100%',
    textAlign: 'left',
    background: 'rgba(0,0,0,0.16)',
    border: '1px solid rgba(255,255,255,0.06)',
    color: 'rgba(255,255,255,0.9)',
    borderRadius: 10,
    padding: '8px 10px',
    cursor: 'pointer',
    fontSize: 12,
    transition: 'all 200ms ease',
  },
  smallListBtnActive: {
    border: '1px solid rgba(99,102,241,0.6)',
    background: 'rgba(99,102,241,0.10)',
  },
  mono: { fontFamily: 'monospace', fontSize: 11, opacity: 0.85 },
}
