import { memo, useCallback } from 'react'
import type { ChatMsg, ChatRunUi } from './types'
import { Button } from '../ui/Button'
import { Input } from '../ui/Input'
import { RunStatusPanel } from './RunStatusPanel'

type ChatPanelProps = {
  chatMsgs: ChatMsg[]
  chatInput: string
  chatBusy: boolean
  chatError: string | null
  chatDebug: boolean
  chatRun: ChatRunUi | null
  uiNowMs: number
  chatPollPaused: boolean
  cardBusy: Record<string, boolean>
  onInputChange: (value: string) => void
  onSend: () => void
  onPausePoll: () => void
  onResumePoll: () => void
  onForceRefresh: () => void
  onCardApproveChangeSet: (changesetId: string) => void
  onCardRejectChangeSet: (changesetId: string) => void
  onCardChooseIntent: (label: string) => void
}

export const ChatPanel = memo(function ChatPanel({
  chatMsgs,
  chatInput,
  chatBusy,
  chatError,
  chatDebug,
  chatRun,
  uiNowMs,
  chatPollPaused,
  cardBusy,
  onInputChange,
  onSend,
  onPausePoll,
  onResumePoll,
  onForceRefresh,
  onCardApproveChangeSet,
  onCardRejectChangeSet,
  onCardChooseIntent,
}: ChatPanelProps) {
  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLInputElement>) => {
      if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
        e.preventDefault()
        onSend()
      }
    },
    [onSend]
  )

  const renderCards = useCallback(
    (cards: any[] | undefined) => {
      if (!cards || !cards.length) return null
      return (
        <div style={{ marginTop: 10, display: 'flex', flexDirection: 'column', gap: 8 }}>
          {cards.map((c, idx) => (
            <div
              key={idx}
              style={{
                border: '1px solid rgba(255,255,255,0.1)',
                borderRadius: 10,
                padding: 10,
                background: 'rgba(0,0,0,0.12)',
              }}
            >
              <div style={{ fontWeight: 700, fontSize: 12 }}>{String(c?.title || c?.type || 'Card')}</div>
              {c?.summary ? (
                <div style={{ marginTop: 6, fontSize: 12, opacity: 0.85, whiteSpace: 'pre-wrap' }}>{String(c.summary)}</div>
              ) : null}
              {c?.hint ? (
                <div style={{ marginTop: 6, fontSize: 11, opacity: 0.65, whiteSpace: 'pre-wrap' }}>{String(c.hint)}</div>
              ) : null}

              {/* 澄清意图卡片 */}
              {c?.type === 'clarify_intent' && Array.isArray(c?.options) ? (
                <div style={{ marginTop: 8, display: 'flex', gap: 8, flexWrap: 'wrap' }}>
                  {c.options.slice(0, 10).map((opt: any, j: number) => (
                    <Button key={j} onClick={() => onCardChooseIntent(String(opt?.label || opt?.value || ''))}>
                      {String(opt?.label || opt?.value || '选项')}
                    </Button>
                  ))}
                </div>
              ) : null}

              {/* 审阅 ChangeSet 卡片 */}
              {c?.type === 'review_changeset' && c?.changeset_id ? (
                <div style={{ marginTop: 8, display: 'flex', gap: 8 }}>
                  <Button
                    variant="primary"
                    disabled={!!cardBusy[`approve:${String(c.changeset_id)}`]}
                    onClick={() => onCardApproveChangeSet(String(c.changeset_id))}
                  >
                    {cardBusy[`approve:${String(c.changeset_id)}`] ? '提交中…' : '确认提交'}
                  </Button>
                  <Button
                    disabled={!!cardBusy[`reject:${String(c.changeset_id)}`]}
                    onClick={() => onCardRejectChangeSet(String(c.changeset_id))}
                  >
                    {cardBusy[`reject:${String(c.changeset_id)}`] ? '驳回中…' : '驳回'}
                  </Button>
                </div>
              ) : null}
            </div>
          ))}
        </div>
      )
    },
    [cardBusy, onCardApproveChangeSet, onCardRejectChangeSet, onCardChooseIntent]
  )

  return (
    <div>
      <div style={{ fontSize: 12, fontWeight: 700, opacity: 0.85, marginBottom: 8 }}>Chat（唯一入口）</div>
      {chatError ? (
        <div style={{ color: '#f87171', fontSize: 12, marginBottom: 8 }} role="alert" aria-live="assertive">
          {String(chatError)}
        </div>
      ) : null}

      {/* 执行步骤（执行中可见） */}
      {chatRun ? (
        <RunStatusPanel
          chatRun={chatRun}
          uiNowMs={uiNowMs}
          onPausePoll={onPausePoll}
          onResumePoll={onResumePoll}
          onForceRefresh={onForceRefresh}
          chatPollPaused={chatPollPaused}
        />
      ) : null}

      <div
        className="aicomic-scroll"
        style={{
          maxHeight: 240,
          overflowY: 'auto',
          background: 'rgba(0,0,0,0.12)',
          border: '1px solid rgba(255,255,255,0.06)',
          borderRadius: 10,
          padding: 10,
          marginBottom: 10,
        }}
      >
        {chatMsgs.length === 0 ? (
          <div style={{ fontSize: 12, opacity: 0.6 }}>
            例：帮我提取本集大纲并优化节奏，减少对白；完成后把变更提交给我确认。
          </div>
        ) : (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
            {chatMsgs.slice(-40).map((m) => (
              <div key={m.id}>
                <div style={{ display: 'flex', gap: 10, alignItems: 'flex-start' }}>
                  <div style={{ width: 48, fontSize: 11, opacity: 0.6 }}>{m.role === 'user' ? '用户' : '系统'}</div>
                  <div style={{ flex: 1, whiteSpace: 'pre-wrap', fontSize: 12, lineHeight: 1.5, opacity: 0.92 }}>
                    {m.content}
                  </div>
                </div>
                {renderCards(m.cards)}
                {chatDebug && m.debug ? (
                  <details style={{ marginTop: 8 }}>
                    <summary style={{ cursor: 'pointer', fontSize: 11, opacity: 0.7 }}>debug</summary>
                    <pre style={{ fontSize: 11, opacity: 0.85, whiteSpace: 'pre-wrap' }}>{JSON.stringify(m.debug, null, 2)}</pre>
                  </details>
                ) : null}
              </div>
            ))}
          </div>
        )}
      </div>

      <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
        <Input
          value={chatInput}
          onChange={(e) => onInputChange(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder="输入你的意图（Ctrl/Cmd + Enter 发送）"
          disabled={chatBusy}
          style={{ flex: 1 }}
        />
        <Button variant="primary" onClick={onSend} disabled={chatBusy || !chatInput.trim()}>
          {chatBusy ? '执行中…' : '发送'}
        </Button>
      </div>
    </div>
  )
})
