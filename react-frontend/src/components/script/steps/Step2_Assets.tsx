import { memo, useState, useMemo } from 'react'
import { SharedStepLayout } from './SharedStepLayout'
import { Button } from '../../ui/Button'

type Step2Props = {
  rawText: string
  busy: boolean
}

export const Step2_Assets = memo(function Step2_Assets({
  rawText,
  busy,
}: Step2Props) {
  const [showCode] = useState(true)

  const assets = useMemo(() => {
    try {
      const parsed = JSON.parse(rawText)
      if (typeof parsed !== 'object' || parsed === null) return []

      // Map top-level keys to cards
      // e.g. { "Characters": "...", "Scenes": "..." }
      if (!Array.isArray(parsed)) {
          return Object.entries(parsed).map(([key, value]) => ({
              title: key,
              content: typeof value === 'string' ? value : JSON.stringify(value, null, 2)
          }))
      }

      // Handle array case
      return parsed.map((item, idx) => ({
          title: item.title || item.name || `Asset ${idx + 1}`,
          content: item.description || item.visual_dna || JSON.stringify(item, null, 2)
      }))
    } catch {
      return []
    }
  }, [rawText])

  const toolbar = null // No actions in Step 2 for now, just viewing/library

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
        Assets JSON
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
      <div style={{ padding: 100, display: 'flex', flexWrap: 'wrap', gap: 20, justifyContent: 'center' }}>
        {assets.length > 0 ? (
          assets.map((asset, idx) => (
            <div key={idx} style={styles.card}>
              <div style={styles.cardHeader}>
                  <div style={styles.cardTitle}>{asset.title}</div>
              </div>
              <div style={styles.cardContent}>{asset.content}</div>
              <div style={styles.cardFooter}>
                  <Button onClick={() => alert('Add to Library logic here')}>入库</Button>
              </div>
            </div>
          ))
        ) : (
          !busy && <div style={{ opacity: 0.5, textAlign: 'center', marginTop: 100 }}>等待提取资产数据...</div>
        )}
      </div>
    </SharedStepLayout>
  )
})

const styles: Record<string, React.CSSProperties> = {
  card: {
    background: 'rgba(255,255,255,0.05)',
    border: '1px solid rgba(255,255,255,0.1)',
    borderRadius: 8,
    padding: 16,
    boxShadow: '0 4px 6px rgba(0,0,0,0.2)',
    display: 'flex',
    flexDirection: 'column',
    gap: 10,
  },
  cardHeader: {
    display: 'flex',
    flexDirection: 'column',
    gap: 4,
  },
  cardType: {
      fontSize: 10,
      textTransform: 'uppercase',
      opacity: 0.5,
      fontWeight: 700,
  },
  cardTitle: {
    fontSize: 14,
    fontWeight: 700,
    color: '#fff',
  },
  cardContent: {
    fontSize: 12,
    opacity: 0.8,
    lineHeight: 1.5,
    whiteSpace: 'pre-wrap',
    flex: 1,
  },
  cardFooter: {
      display: 'flex',
      justifyContent: 'flex-end',
  }
}
