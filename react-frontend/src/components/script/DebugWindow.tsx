import { memo, useRef, useEffect } from 'react'
import type { DebugLog, ChatRunUi } from './types'
import { Button } from '../ui/Button'

type DebugWindowProps = {
  show: boolean
  debugLogs: DebugLog[]
  chatRun: ChatRunUi | null
  autoScrollEnabled: boolean
  hasNewLogs: boolean
  onToggle: () => void
  onClear: () => void
  onAutoScrollToggle: () => void
  onScrollToBottom: () => void
}

export const DebugWindow = memo(function DebugWindow({
  show,
  debugLogs,
  chatRun,
  autoScrollEnabled,
  hasNewLogs,
  onToggle,
  onClear,
  onAutoScrollToggle,
  onScrollToBottom,
}: DebugWindowProps) {
  const logsContainerRef = useRef<HTMLDivElement>(null)
  const logsEndRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (autoScrollEnabled && logsEndRef.current) {
      logsEndRef.current.scrollIntoView({ behavior: 'smooth' })
    }
  }, [debugLogs, autoScrollEnabled])

  const handleScroll = () => {
    if (!logsContainerRef.current) return
    const { scrollTop, scrollHeight, clientHeight } = logsContainerRef.current
    const isAtBottom = scrollHeight - scrollTop - clientHeight < 50
    if (!isAtBottom && autoScrollEnabled) {
      onAutoScrollToggle()
    }
  }

  return (
    <>
      {/* 悬浮开关按钮 */}
      <div style={{ position: 'fixed', bottom: 20, right: 20, zIndex: 1000 }}>
        <Button
          onClick={onToggle}
          style={{
            background: '#111',
            border: '1px solid #444',
            boxShadow: '0 4px 12px rgba(0,0,0,0.5)',
            display: 'flex',
            alignItems: 'center',
            gap: 6,
          }}
        >
          <span>🐞 Debug</span>
          {debugLogs.length > 0 && (
            <span
              style={{
                background: '#ef4444',
                color: 'white',
                borderRadius: 10,
                padding: '0 5px',
                fontSize: 10,
                minWidth: 16,
                textAlign: 'center',
              }}
            >
              {debugLogs.length}
            </span>
          )}
        </Button>
      </div>

      {/* Debug 窗口面板 */}
      {show && (
        <div
          style={{
            position: 'fixed',
            bottom: 60,
            right: 20,
            width: 600,
            height: 400,
            background: 'rgba(15,15,15,0.98)',
            border: '1px solid #444',
            borderRadius: 8,
            display: 'flex',
            flexDirection: 'column',
            boxShadow: '0 8px 30px rgba(0,0,0,0.7)',
            zIndex: 1000,
            overflow: 'hidden',
            backdropFilter: 'blur(10px)',
          }}
        >
          {/* 标题栏 */}
          <div
            style={{
              padding: '8px 12px',
              background: '#222',
              borderBottom: '1px solid #333',
              fontSize: 12,
              fontWeight: 700,
              color: '#aaa',
              display: 'flex',
              justifyContent: 'space-between',
              alignItems: 'center',
            }}
          >
            <div style={{ display: 'flex', gap: 10 }}>
              <span>后端实时日志</span>
              <span style={{ opacity: 0.5 }}>Run: {chatRun?.runId ? chatRun.runId.slice(0, 8) : '-'}</span>
            </div>
            <div style={{ display: 'flex', gap: 10 }}>
              <div
                style={{
                  fontSize: 10,
                  cursor: 'pointer',
                  color: autoScrollEnabled ? '#34d399' : '#aaa',
                  display: 'flex',
                  alignItems: 'center',
                }}
                onClick={onAutoScrollToggle}
                role="button"
                tabIndex={0}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' || e.key === ' ') {
                    e.preventDefault()
                    onAutoScrollToggle()
                  }
                }}
              >
                {autoScrollEnabled ? '🟢 自动滚动' : '⚪️ 已暂停滚动'}
              </div>
              <button
                onClick={onClear}
                style={{ background: 'none', border: 'none', color: '#aaa', cursor: 'pointer', fontSize: 11 }}
              >
                清空
              </button>
              <button
                onClick={onToggle}
                style={{ background: 'none', border: 'none', color: '#aaa', cursor: 'pointer', fontSize: 14 }}
              >
                ×
              </button>
            </div>
          </div>

          {/* 日志内容区域 */}
          <div
            ref={logsContainerRef}
            onScroll={handleScroll}
            style={{
              flex: 1,
              overflowY: 'auto',
              padding: 12,
              fontFamily: 'Menlo, Monaco, "Courier New", monospace',
              fontSize: 11,
              lineHeight: '1.5',
              color: '#e5e5e5',
              position: 'relative',
            }}
          >
            {debugLogs.length === 0 ? (
              <div style={{ opacity: 0.3, textAlign: 'center', marginTop: 40 }}>暂无日志...</div>
            ) : null}

            {debugLogs.map((log) => (
              <div
                key={log.id}
                style={{
                  marginBottom: 6,
                  display: 'flex',
                  gap: 8,
                  alignItems: 'flex-start',
                  borderBottom: '1px solid rgba(255,255,255,0.03)',
                  paddingBottom: 4,
                }}
              >
                <span style={{ opacity: 0.4, minWidth: 60, fontSize: 10, paddingTop: 1 }}>
                  {new Date(log.ts).toLocaleTimeString([], {
                    hour12: false,
                    hour: '2-digit',
                    minute: '2-digit',
                    second: '2-digit',
                  })}
                </span>
                <span
                  style={{
                    color: log.level === 'ERROR' ? '#f87171' : log.level === 'SUCCESS' ? '#34d399' : '#60a5fa',
                    fontWeight: 'bold',
                    minWidth: 45,
                    fontSize: 10,
                    paddingTop: 1,
                  }}
                >
                  [{log.level}]
                </span>
                <span style={{ whiteSpace: 'pre-wrap', wordBreak: 'break-all', flex: 1 }}>{log.text}</span>
              </div>
            ))}

            {/* 滚动锚点 */}
            <div ref={logsEndRef} />

            {/* 底部新消息提示浮层 */}
            {!autoScrollEnabled && hasNewLogs && (
              <div
                onClick={() => {
                  onScrollToBottom()
                  onAutoScrollToggle()
                }}
                style={{
                  position: 'sticky',
                  bottom: 10,
                  left: '50%',
                  transform: 'translateX(-50%)',
                  background: '#6366f1',
                  color: 'white',
                  padding: '4px 12px',
                  borderRadius: 20,
                  fontSize: 11,
                  cursor: 'pointer',
                  boxShadow: '0 4px 12px rgba(0,0,0,0.5)',
                  fontWeight: 600,
                }}
                role="button"
                tabIndex={0}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' || e.key === ' ') {
                    e.preventDefault()
                    onScrollToBottom()
                    onAutoScrollToggle()
                  }
                }}
              >
                ⬇️ 有新日志
              </div>
            )}
          </div>
        </div>
      )}
    </>
  )
})
