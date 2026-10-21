import { memo, useState, useMemo } from 'react'
import { SharedStepLayout, CanvasDraggablePanel } from './SharedStepLayout'
import { Button } from '../../ui/Button'

type Step1Props = {
  rawText: string
  onNext: () => void
  busy: boolean
}

export const Step1_Structure = memo(function Step1_Structure({
  rawText,
  onNext,
  busy,
}: Step1Props) {
  const [showCode] = useState(true)

  const cards = useMemo(() => {
    try {
      const parsed = JSON.parse(rawText)
      if (typeof parsed !== 'object' || parsed === null) return []
      
      // Map top-level keys to cards
      // e.g. { "Act 1": "...", "Act 2": "..." }
      if (!Array.isArray(parsed)) {
          return Object.entries(parsed).map(([key, value]) => ({
              title: key,
              content: typeof value === 'string' ? value : JSON.stringify(value, null, 2)
          }))
      }
      
      // Handle array case
      return parsed.map((item, idx) => ({
          title: item.title || item.name || `Node ${idx + 1}`,
          content: item.description || item.summary || JSON.stringify(item, null, 2)
      }))
    } catch {
      return []
    }
  }, [rawText])

  const toolbar = (
    <Button variant="primary" onClick={onNext} disabled={busy}>
      {busy ? '执行中...' : '资产抽离 →'}
    </Button>
  )

  const codeWindow = (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
      <div
        data-aicomic-drag-handle="true"
        style={{
          padding: '8px 12px',
          background: 'rgba(255,255,255,0.05)',
          borderBottom: '1px solid rgba(255,255,255,0.1)',
          fontSize: 12,
          fontWeight: 700,
          cursor: 'grab',
          userSelect: 'none',
          touchAction: 'none',
        }}
      >
        Structure JSON
      </div>
      <textarea
        value={rawText}
        readOnly
        style={{
          flex: 1,
          background: 'transparent',
          border: 'none',
          color: '#e5e7eb',
          padding: 10,
          fontSize: 12,
          fontFamily: 'Menlo, Monaco, monospace',
          resize: 'none',
          outline: 'none',
        }}
      />
    </div>
  )

  return (
    <SharedStepLayout 
      toolbar={toolbar}
      codeWindow={showCode ? codeWindow : null}
      busy={busy}
      enableInfiniteCanvas={true}
    >
      {cards.length > 0 ? (
        <div style={styles.board}>
          {cards.map((card, idx) => {
            const col = idx % 3
            const row = Math.floor(idx / 3)
            const left = 120 + col * 360
            const top = 120 + row * 260
            return (
              <CanvasDraggablePanel
                key={idx}
                disabled={busy}
                panelStyle={{
                  ...styles.card,
                  position: 'absolute',
                  left,
                  top,
                }}
              >
                <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
                  <div data-aicomic-drag-handle="true" style={styles.cardTitleBar}>
                    {card.title}
                  </div>
                  <div style={styles.cardContent}>{card.content}</div>
                </div>
              </CanvasDraggablePanel>
            )
          })}
        </div>
      ) : (
        !busy && <div style={{ opacity: 0.5, marginTop: 100, padding: 100 }}>等待生成结构化数据...</div>
      )}
    </SharedStepLayout>
  )
})

const styles: Record<string, React.CSSProperties> = {
  board: {
    position: 'relative',
    width: 1400,
    height: 1000,
  },
  card: {
    width: 300,
    background: 'rgba(255,255,255,0.05)',
    border: '1px solid rgba(255,255,255,0.1)',
    borderRadius: 8,
    boxShadow: '0 4px 6px rgba(0,0,0,0.2)',
    overflow: 'hidden',
  },
  cardTitleBar: {
    padding: '10px 12px',
    fontSize: 14,
    fontWeight: 700,
    color: '#fff',
    cursor: 'grab',
    userSelect: 'none',
    touchAction: 'none',
    background: 'rgba(255,255,255,0.04)',
    borderBottom: '1px solid rgba(255,255,255,0.08)',
  },
  cardContent: {
    padding: 12,
    fontSize: 12,
    opacity: 0.8,
    lineHeight: 1.5,
    whiteSpace: 'pre-wrap',
    maxHeight: 240,
    overflow: 'auto',
  }
}
