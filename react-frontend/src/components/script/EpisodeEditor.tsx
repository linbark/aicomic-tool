import { memo } from 'react'
import type { EpisodeRead } from '../../api/types'
import { Button } from '../ui/Button'
import { Textarea } from '../ui/Textarea'
import { panelStyle } from '../../styles/shared'
import type { ChatRunUi } from './types'
import { ExecutionPanel } from './ExecutionPanel'

type EpisodeEditorProps = {
  episode: EpisodeRead
  episodeText: string
  uiNowMs: number
  execRun: ChatRunUi | null
  execPollPaused: boolean
  interruptKind: string | null
  execBusy: boolean
  busy: boolean
  deleting: boolean
  rawAssetsVisualDnaText?: string
  rawSplitEpisodesText?: string
  onEpisodeTextChange: (value: string) => void
  onSave: () => void
  onCreateScene: () => void
  onDelete: () => void
  onExecute: () => void
  onPauseExecPoll: () => void
  onResumeExecPoll: () => void
  onForceRefreshExec: () => void
  onConfirmExec: (decision: 'confirmed' | 'regenerate' | 'rejected', artifacts?: Record<string, unknown>) => void
}

export const EpisodeEditor = memo(function EpisodeEditor({
  episode,
  episodeText,
  uiNowMs,
  execRun,
  execPollPaused,
  interruptKind,
  execBusy,
  busy,
  deleting,
  rawAssetsVisualDnaText,
  rawSplitEpisodesText,
  onEpisodeTextChange,
  onSave,
  onCreateScene,
  onDelete,
  onExecute,
  onPauseExecPoll,
  onResumeExecPoll,
  onForceRefreshExec,
  onConfirmExec,
}: EpisodeEditorProps) {
  return (
    <section style={panelStyle}>
      <div style={styles.panelHeader}>
        <div style={{ fontWeight: 700, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis' }}>
          <span style={styles.mono}>EP{episode.order}</span> {episode.title}
        </div>
        <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
          <Button onClick={onCreateScene} disabled={busy}>
            + 新建场
          </Button>
          <Button variant="primary" onClick={onSave} disabled={busy || !!episode.script_locked}>
            保存本集
          </Button>
          <Button onClick={onDelete} disabled={deleting}>
            {deleting ? '删除中…' : '删除本集'}
          </Button>
        </div>
      </div>

      <div style={styles.labelRow}>
        <div style={styles.label}>当前剧本</div>
      </div>
      <Textarea
        value={episodeText}
        onChange={(e) => onEpisodeTextChange(e.target.value)}
        style={{ height: 220 }}
        placeholder="本集剧本内容…"
        disabled={!!episode.script_locked}
      />

      <div style={{ borderTop: '1px solid rgba(255,255,255,0.1)', margin: '12px 0' }} />

      <ExecutionPanel
        key={episode.id}
        episode={episode}
        execRun={execRun}
        uiNowMs={uiNowMs}
        execPollPaused={execPollPaused}
        interruptKind={interruptKind}
        execBusy={execBusy}
        disableExecute={busy}
        rawAssetsVisualDnaText={rawAssetsVisualDnaText || ''}
        rawSplitEpisodesText={rawSplitEpisodesText || ''}
        onExecute={onExecute}
        onPausePoll={onPauseExecPoll}
        onResumePoll={onResumeExecPoll}
        onForceRefresh={onForceRefreshExec}
        onConfirm={onConfirmExec}
      />
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
