import { memo } from 'react'
import type { ChatRunUi } from './types'
import { Button } from '../ui/Button'
import { trim } from './utils'
import { panelStyle } from '../../styles/shared'

type RunStatusPanelProps = {
  chatRun: ChatRunUi
  uiNowMs: number
  onPausePoll: () => void
  onResumePoll: () => void
  onForceRefresh: () => void
  chatPollPaused: boolean
}

export const RunStatusPanel = memo(function RunStatusPanel({
  chatRun,
  uiNowMs,
  onPausePoll,
  onResumePoll,
  onForceRefresh,
  chatPollPaused,
}: RunStatusPanelProps) {
  const last = typeof chatRun.lastAtMs === 'number' ? chatRun.lastAtMs : null
  const idleMs = last ? uiNowMs - last : 0
  const showIdleWarning = idleMs >= 120_000

  return (
    <div style={{ ...panelStyle, marginBottom: 10 }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
        <div style={{ fontSize: 12, fontWeight: 700, opacity: 0.85 }}>执行步骤</div>
        <div style={{ fontSize: 11, opacity: 0.65 }}>
          状态：{chatRun.status} {chatRun.runId ? `（${chatRun.runId.slice(0, 8)}）` : ''}
        </div>
      </div>

      <div style={{ marginTop: 6, display: 'flex', justifyContent: 'space-between', gap: 10, alignItems: 'center' }}>
        <div style={{ fontSize: 11, opacity: 0.7 }}>
          当前：{chatRun.currentActionKey || (chatRun.currentStepIndex != null ? `step_${chatRun.currentStepIndex + 1}` : '—')}
          {typeof chatRun.startedAtMs === 'number' ? `｜已运行 ${(Math.max(0, uiNowMs - chatRun.startedAtMs) / 1000).toFixed(0)}s` : ''}
          {last ? `｜最近更新 ${(Math.max(0, uiNowMs - last) / 1000).toFixed(0)}s 前` : ''}
        </div>
        <div style={{ display: 'flex', gap: 8 }}>
          {chatRun.status === 'running' || chatRun.status === 'queued' ? (
            chatPollPaused ? (
              <Button onClick={onResumePoll}>继续轮询</Button>
            ) : (
              <Button onClick={onPausePoll}>暂停轮询</Button>
            )
          ) : null}
        </div>
      </div>

      {showIdleWarning && (
        <div style={{ marginTop: 8, fontSize: 12, color: idleMs >= 300_000 ? '#fca5a5' : '#fbbf24' }}>
          {idleMs >= 300_000
            ? '超过 5 分钟无进展更新，可能卡住或网络较慢。后端可能仍在执行，你可以继续等待，或暂停轮询后稍后再继续。'
            : '超过 2 分钟无进展更新（可能在长耗时步骤中）。如需，可暂停轮询后稍后再继续。'}
          <Button
            onClick={onForceRefresh}
            style={{ marginLeft: 8, fontSize: 11, padding: '2px 8px' }}
          >
            强制刷新状态
          </Button>
        </div>
      )}

      {chatRun.error ? (
        <div style={{ marginTop: 6, color: '#f87171', fontSize: 12 }} role="alert">
          {chatRun.error}
        </div>
      ) : null}

      {chatRun.steps.length ? (
        <div style={{ marginTop: 8, display: 'flex', flexDirection: 'column', gap: 6 }}>
          {chatRun.steps.map((s) => (
            <div key={s.index} style={{ display: 'flex', gap: 10, alignItems: 'flex-start' }}>
              <div style={{ width: 22, opacity: 0.6, fontFamily: 'monospace' }}>{s.index + 1}</div>
              <div style={{ flex: 1 }}>
                <div style={{ fontSize: 12, opacity: 0.92 }}>
                  {s.action_key} <span style={{ opacity: 0.65 }}>[{s.status}{typeof s.ms === 'number' ? ` ${s.ms}ms` : ''}]</span>
                </div>
                {s.why ? <div style={{ fontSize: 11, opacity: 0.65, whiteSpace: 'pre-wrap' }}>{s.why}</div> : null}
                {s.output_preview ? (
                  <div style={{ marginTop: 4, fontSize: 11, opacity: 0.7, whiteSpace: 'pre-wrap' }}>
                    {trim(s.output_preview, 500)}
                  </div>
                ) : null}
              </div>
            </div>
          ))}
        </div>
      ) : (
        <div style={{ marginTop: 8, fontSize: 12, opacity: 0.6 }}>规划中…</div>
      )}
    </div>
  )
})
