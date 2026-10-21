import { memo } from 'react'
import type { SceneRead } from '../../api/types'
import { Button } from '../ui/Button'
import { Textarea } from '../ui/Textarea'
import { panelStyle } from '../../styles/shared'

type SceneEditorProps = {
  scene: SceneRead
  sceneText: string
  busy: boolean
  onSceneTextChange: (value: string) => void
  onCreateShot: () => void
  onSave: () => void
  onSelectShot: (shotId: number) => void
  selectedShotId: number | null
}

export const SceneEditor = memo(function SceneEditor({
  scene,
  sceneText,
  busy,
  onSceneTextChange,
  onCreateShot,
  onSave,
  onSelectShot,
  selectedShotId,
}: SceneEditorProps) {
  return (
    <section style={panelStyle}>
      <div style={styles.panelHeader}>
        <div style={{ fontWeight: 700 }}>
          <span style={styles.mono}>SC{scene.sequence_number ?? ''}</span> {scene.title || '未命名场景'}
        </div>
        <div style={{ display: 'flex', gap: 8 }}>
          <Button onClick={onCreateShot} disabled={busy}>
            + 新建镜头
          </Button>
          <Button variant="primary" onClick={onSave} disabled={busy}>
            保存本场
          </Button>
        </div>
      </div>

      <div style={styles.labelRow}>
        <div style={styles.label}>场景内容</div>
        <div style={{ fontSize: 12, opacity: 0.5 }}>AI 功能已下线；请在 Episode Chat 中驱动工作流</div>
      </div>
      <Textarea value={sceneText} onChange={(e) => onSceneTextChange(e.target.value)} style={{ height: 220 }} />

      <div style={{ borderTop: '1px solid rgba(255,255,255,0.1)', margin: '12px 0' }} />
      <div style={{ fontSize: 12, fontWeight: 700, opacity: 0.85, marginBottom: 8 }}>镜头列表</div>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
        {(scene.shots || []).map((sh) => (
          <button
            key={sh.id}
            style={{
              ...styles.smallListBtn,
              ...(selectedShotId === sh.id ? styles.smallListBtnActive : null),
            }}
            onClick={() => onSelectShot(sh.id)}
            onFocus={(e) => {
              e.currentTarget.style.outline = '2px solid rgba(99,102,241,0.6)'
              e.currentTarget.style.outlineOffset = '2px'
            }}
            onBlur={(e) => {
              e.currentTarget.style.outline = 'none'
            }}
          >
            <span style={styles.mono}>SH{sh.sequence_number ?? ''}</span> {sh.title || '未命名镜头'}
          </button>
        ))}
        {!(scene.shots || []).length ? <div style={{ fontSize: 12, opacity: 0.6 }}>暂无镜头</div> : null}
      </div>
    </section>
  )
})

const styles: Record<string, React.CSSProperties> = {
  panelHeader: {
    display: 'flex',
    justifyContent: 'space-between',
    alignItems: 'center',
    gap: 10,
    marginBottom: 10,
  },
  labelRow: {
    display: 'flex',
    justifyContent: 'space-between',
    alignItems: 'center',
    marginBottom: 6,
  },
  label: {
    fontSize: 12,
    fontWeight: 700,
    opacity: 0.85,
  },
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
