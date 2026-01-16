import { memo, useState, useMemo } from 'react'
import { SharedStepLayout } from './SharedStepLayout'
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
      <div
        style={{
          padding: 100,
          minHeight: '100%',
          display: 'flex',
          flexWrap: 'wrap',
          gap: 20,
          justifyContent: 'center',
          width: 1200,
          maxWidth: '100%',
          margin: '0 auto',
        }}
      >
        {cards.length > 0 ? (
          cards.map((card, idx) => (
            <div key={idx} style={styles.card}>
              <div style={styles.cardTitle}>{card.title}</div>
              <div style={styles.cardContent}>{card.content}</div>
            </div>
          ))
        ) : (
          !busy && <div style={{ opacity: 0.5, marginTop: 100 }}>等待生成结构化数据...</div>
        )}
      </div>
    </SharedStepLayout>
  )
})

const styles: Record<string, React.CSSProperties> = {
  card: {
    width: 300,
    background: 'rgba(255,255,255,0.05)',
    border: '1px solid rgba(255,255,255,0.1)',
    borderRadius: 8,
    padding: 16,
    boxShadow: '0 4px 6px rgba(0,0,0,0.2)',
  },
  cardTitle: {
    fontSize: 14,
    fontWeight: 700,
    marginBottom: 8,
    color: '#fff',
  },
  cardContent: {
    fontSize: 12,
    opacity: 0.8,
    lineHeight: 1.5,
    whiteSpace: 'pre-wrap',
  }
}
