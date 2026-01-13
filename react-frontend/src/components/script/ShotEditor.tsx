import { memo } from 'react'
import type { ShotRead } from '../../api/types'
import { Button } from '../ui/Button'
import { Input } from '../ui/Input'
import { Textarea } from '../ui/Textarea'
import { panelStyle } from '../../styles/shared'

type ShotEditorProps = {
  shot: ShotRead
  shotDraft: Partial<ShotRead>
  busy: boolean
  onDraftChange: (draft: Partial<ShotRead>) => void
  onSave: () => void
}

export const ShotEditor = memo(function ShotEditor({ shot, shotDraft, busy, onDraftChange, onSave }: ShotEditorProps) {
  return (
    <section style={panelStyle}>
      <div style={styles.panelHeader}>
        <div style={{ fontWeight: 700 }}>
          <span style={styles.mono}>SH{shot.sequence_number ?? ''}</span> {shot.title || '未命名镜头'}
        </div>
        <Button variant="primary" onClick={onSave} disabled={busy}>
          保存镜头
        </Button>
      </div>

      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 10 }}>
        <div>
          <div style={styles.labelRow}>
            <div style={styles.label}>标题</div>
          </div>
          <Input
            value={String(shotDraft.title || '')}
            onChange={(e) => onDraftChange({ ...shotDraft, title: e.target.value })}
            aria-label="镜头标题"
          />
        </div>
        <div>
          <div style={styles.labelRow}>
            <div style={styles.label}>对白（可选）</div>
          </div>
          <Input
            value={String(shotDraft.dialogue || '')}
            onChange={(e) => onDraftChange({ ...shotDraft, dialogue: e.target.value })}
            aria-label="镜头对白"
          />
        </div>
      </div>

      <div style={{ marginTop: 10 }}>
        <div style={styles.labelRow}>
          <div style={styles.label}>动作</div>
        </div>
        <Textarea
          value={String(shotDraft.action_text || '')}
          onChange={(e) => onDraftChange({ ...shotDraft, action_text: e.target.value })}
          style={{ height: 160 }}
          aria-label="镜头动作"
        />
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
  mono: { fontFamily: 'monospace', fontSize: 11, opacity: 0.85 },
}
