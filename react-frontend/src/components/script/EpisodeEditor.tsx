import { memo } from 'react'
import type { EpisodeRead } from '../../api/types'
import type { ChatMsg, ChatRunUi } from './types'
import { Button } from '../ui/Button'
import { Textarea } from '../ui/Textarea'
import { ChatPanel } from './ChatPanel'
import { panelStyle } from '../../styles/shared'

type EpisodeEditorProps = {
  episode: EpisodeRead
  episodeText: string
  chatMsgs: ChatMsg[]
  chatInput: string
  chatBusy: boolean
  chatError: string | null
  chatDebug: boolean
  chatRun: ChatRunUi | null
  uiNowMs: number
  chatPollPaused: boolean
  cardBusy: Record<string, boolean>
  busy: boolean
  deleting: boolean
  onEpisodeTextChange: (value: string) => void
  onChatInputChange: (value: string) => void
  onDebugChange: (value: boolean) => void
  onSave: () => void
  onCreateScene: () => void
  onDelete: () => void
  onSendChat: () => void
  onPausePoll: () => void
  onResumePoll: () => void
  onForceRefresh: () => void
  onCardApproveChangeSet: (changesetId: string) => void
  onCardRejectChangeSet: (changesetId: string) => void
  onCardChooseIntent: (label: string) => void
}

export const EpisodeEditor = memo(function EpisodeEditor({
  episode,
  episodeText,
  chatMsgs,
  chatInput,
  chatBusy,
  chatError,
  chatDebug,
  chatRun,
  uiNowMs,
  chatPollPaused,
  cardBusy,
  busy,
  deleting,
  onEpisodeTextChange,
  onChatInputChange,
  onDebugChange,
  onSave,
  onCreateScene,
  onDelete,
  onSendChat,
  onPausePoll,
  onResumePoll,
  onForceRefresh,
  onCardApproveChangeSet,
  onCardRejectChangeSet,
  onCardChooseIntent,
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
          <Button variant="primary" onClick={onSave} disabled={busy}>
            保存本集
          </Button>
          <Button onClick={onDelete} disabled={deleting}>
            {deleting ? '删除中…' : '删除本集'}
          </Button>
        </div>
      </div>

      <div style={{ marginBottom: 10, fontSize: 12, opacity: 0.7 }}>
        Episode-详情已改为 Chat 驱动：请直接对话提出意图（生成/优化/分镜/入库审阅），系统会自动规划执行路径。
      </div>

      <div style={styles.labelRow}>
        <div style={styles.label}>当前剧本（可选：直接编辑）</div>
        <label style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: 11, opacity: 0.75, cursor: 'pointer' }}>
          <input type="checkbox" checked={chatDebug} onChange={(e) => onDebugChange(e.target.checked)} />
          Debug
        </label>
      </div>
      <Textarea
        value={episodeText}
        onChange={(e) => onEpisodeTextChange(e.target.value)}
        style={{ height: 220 }}
        placeholder="本集剧本内容…"
      />

      <div style={{ borderTop: '1px solid rgba(255,255,255,0.1)', margin: '12px 0' }} />

      <ChatPanel
        chatMsgs={chatMsgs}
        chatInput={chatInput}
        chatBusy={chatBusy}
        chatError={chatError}
        chatDebug={chatDebug}
        chatRun={chatRun}
        uiNowMs={uiNowMs}
        chatPollPaused={chatPollPaused}
        cardBusy={cardBusy}
        onInputChange={onChatInputChange}
        onSend={onSendChat}
        onPausePoll={onPausePoll}
        onResumePoll={onResumePoll}
        onForceRefresh={onForceRefresh}
        onCardApproveChangeSet={onCardApproveChangeSet}
        onCardRejectChangeSet={onCardRejectChangeSet}
        onCardChooseIntent={onCardChooseIntent}
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
