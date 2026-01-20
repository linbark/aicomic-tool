import { memo, useState, useMemo } from 'react'
import { SharedStepLayout } from './SharedStepLayout'
import { Button } from '../../ui/Button'

type Step2Props = {
  rawText: string
  busy: boolean
}

type AssetCard = {
  type: 'character' | 'prop' | 'location' | 'series_style' | 'generic'
  title: string
  description?: string
  visualDna?: Record<string, string>
  tags?: string
  lightingStyle?: string
  cameraLanguage?: string
  composition?: string
  colorPalette?: string[]
  content?: string
}

export const Step2_Assets = memo(function Step2_Assets({
  rawText,
  busy,
}: Step2Props) {
  const [showCode] = useState(true)

  const assets = useMemo<AssetCard[]>(() => {
    try {
      const parsed = JSON.parse(rawText)
      if (typeof parsed !== 'object' || parsed === null) return []

      const toStringValue = (value: unknown) => {
        if (typeof value === 'string') return value
        if (value == null) return ''
        return JSON.stringify(value, null, 2)
      }
      const toContent = (value: unknown) => (typeof value === 'string' ? value : JSON.stringify(value, null, 2))
      const toItemContent = (item: any) => {
        if (item && typeof item === 'object') {
          const picked = item.description ?? item.visual_dna ?? item.stable_diffusion_tags ?? item.prompt ?? item
          return toContent(picked)
        }
        if (item == null) return ''
        return String(item)
      }

      const normalizeVisualDna = (value: unknown) => {
        if (!value || typeof value !== 'object') return null
        return Object.fromEntries(
          Object.entries(value as Record<string, unknown>).map(([key, val]) => [key, toStringValue(val)])
        )
      }
      const isCharacterLike = (item: any) =>
        Boolean(item?.visual_dna || item?.stable_diffusion_tags || (item?.name && item?.description))

      if (!Array.isArray(parsed)) {
        const cards: AssetCard[] = []
        const characters = (parsed as any).characters
        if (Array.isArray(characters)) {
          characters.forEach((item, idx) => {
            const title = String(item?.name || item?.title || `Character ${idx + 1}`)
            const description = typeof item?.description === 'string' ? item.description : ''
            const visualDna = normalizeVisualDna(item?.visual_dna) ?? undefined
            const tags = typeof item?.stable_diffusion_tags === 'string' ? item.stable_diffusion_tags : ''
            cards.push({ type: 'character', title, description, visualDna, tags })
          })
        }
        const props = (parsed as any).props
        if (Array.isArray(props)) {
          props.forEach((item, idx) => {
            const title = String(item?.name || item?.title || `Prop ${idx + 1}`)
            const description = typeof item?.description === 'string' ? item.description : ''
            const tags = typeof item?.stable_diffusion_tags === 'string' ? item.stable_diffusion_tags : ''
            cards.push({ type: 'prop', title, description, tags })
          })
        }
        const locations = (parsed as any).locations
        if (Array.isArray(locations)) {
          locations.forEach((item, idx) => {
            const title = String(item?.name || item?.title || `Location ${idx + 1}`)
            const description = typeof item?.description === 'string' ? item.description : ''
            const tags = typeof item?.stable_diffusion_tags === 'string' ? item.stable_diffusion_tags : ''
            cards.push({ type: 'location', title, description, tags })
          })
        }
        const seriesStyle = (parsed as any).series_style
        if (seriesStyle && typeof seriesStyle === 'object') {
          const title = String(seriesStyle?.name || seriesStyle?.title || 'Series Style')
          const lightingStyle = typeof seriesStyle?.lighting_style === 'string' ? seriesStyle.lighting_style : ''
          const cameraLanguage = typeof seriesStyle?.camera_language === 'string' ? seriesStyle.camera_language : ''
          const composition = typeof seriesStyle?.composition === 'string' ? seriesStyle.composition : ''
          const colorPalette = Array.isArray(seriesStyle?.color_palette)
            ? seriesStyle.color_palette.filter((color: unknown) => typeof color === 'string')
            : []
          const tags = typeof seriesStyle?.stable_diffusion_tags === 'string' ? seriesStyle.stable_diffusion_tags : ''
          cards.push({ type: 'series_style', title, lightingStyle, cameraLanguage, composition, colorPalette, tags })
        }
        Object.entries(parsed).forEach(([key, value]) => {
          if (key === 'characters' || key === 'props' || key === 'locations' || key === 'series_style') return
          cards.push({ type: 'generic', title: key, content: toContent(value) })
        })
        return cards
      }

      return parsed.map((item, idx) => {
        const title = item?.title || item?.name || `Asset ${idx + 1}`
        if (isCharacterLike(item)) {
          const description = typeof item?.description === 'string' ? item.description : ''
          const visualDna = normalizeVisualDna(item?.visual_dna) ?? undefined
          const tags = typeof item?.stable_diffusion_tags === 'string' ? item.stable_diffusion_tags : ''
          return { type: 'character', title, description, visualDna, tags }
        }
        return { type: 'generic', title, content: toItemContent(item) }
      })
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
          assets.map((asset, idx) => {
            if (asset.type === 'character') {
              const visualDnaEntries = asset.visualDna ? Object.entries(asset.visualDna) : []
              return (
                <div key={idx} style={styles.characterCard}>
                  <div style={styles.characterHeader}>
                    <div style={styles.characterName}>{asset.title}</div>
                    <div style={styles.characterBadge}>人物卡</div>
                  </div>
                  {asset.description ? <div style={styles.characterDesc}>{asset.description}</div> : null}
                  {visualDnaEntries.length > 0 ? (
                    <div style={styles.characterDnaGrid}>
                      {visualDnaEntries.map(([key, value]) => (
                        <div key={key} style={styles.dnaItem}>
                          <div style={styles.dnaLabel}>
                            {String(key).replace(/_/g, ' ').replace(/^\w/, (s) => s.toUpperCase())}
                          </div>
                          <div style={styles.dnaValue}>{value}</div>
                        </div>
                      ))}
                    </div>
                  ) : null}
                  {asset.tags ? (
                    <div style={styles.tagBlock}>
                      <div style={styles.tagTitle}>Stable Diffusion Tags</div>
                      <div style={styles.tagContent}>{asset.tags}</div>
                    </div>
                  ) : null}
                  <div style={styles.cardFooter}>
                    <Button onClick={() => alert('Add to Library logic here')}>入库</Button>
                  </div>
                </div>
              )
            }
            if (asset.type === 'prop') {
              return (
                <div key={idx} style={styles.propCard}>
                  <div style={styles.propHeader}>
                    <div style={styles.propIcon}>道具</div>
                    <div style={styles.propName}>{asset.title}</div>
                  </div>
                  {asset.description ? <div style={styles.propDesc}>{asset.description}</div> : null}
                  {asset.tags ? (
                    <div style={styles.propTagBox}>
                      <div style={styles.propTagTitle}>Stable Diffusion Tags</div>
                      <div style={styles.propTagContent}>{asset.tags}</div>
                    </div>
                  ) : null}
                  <div style={styles.cardFooter}>
                    <Button onClick={() => alert('Add to Library logic here')}>入库</Button>
                  </div>
                </div>
              )
            }
            if (asset.type === 'location') {
              return (
                <div key={idx} style={styles.locationCard}>
                  <div style={styles.locationHeader}>
                    <div style={styles.locationTitle}>{asset.title}</div>
                    <div style={styles.locationAccent} />
                  </div>
                  {asset.description ? <div style={styles.locationDesc}>{asset.description}</div> : null}
                  {asset.tags ? (
                    <div style={styles.locationTagBox}>
                      <div style={styles.locationTagTitle}>Stable Diffusion Tags</div>
                      <div style={styles.locationTagContent}>{asset.tags}</div>
                    </div>
                  ) : null}
                  <div style={styles.cardFooter}>
                    <Button onClick={() => alert('Add to Library logic here')}>入库</Button>
                  </div>
                </div>
              )
            }
            if (asset.type === 'series_style') {
              const palette = asset.colorPalette ?? []
              return (
                <div key={idx} style={styles.seriesCard}>
                  <div style={styles.seriesHeader}>
                    <div style={styles.seriesTitle}>{asset.title}</div>
                    <div style={styles.seriesBadge}>风格设定</div>
                  </div>
                  <div style={styles.seriesBody}>
                    {asset.lightingStyle ? (
                      <div style={styles.seriesRow}>
                        <div style={styles.seriesLabel}>Lighting</div>
                        <div style={styles.seriesValue}>{asset.lightingStyle}</div>
                      </div>
                    ) : null}
                    {asset.cameraLanguage ? (
                      <div style={styles.seriesRow}>
                        <div style={styles.seriesLabel}>Camera</div>
                        <div style={styles.seriesValue}>{asset.cameraLanguage}</div>
                      </div>
                    ) : null}
                    {asset.composition ? (
                      <div style={styles.seriesRow}>
                        <div style={styles.seriesLabel}>Composition</div>
                        <div style={styles.seriesValue}>{asset.composition}</div>
                      </div>
                    ) : null}
                  </div>
                  {palette.length > 0 ? (
                    <div style={styles.paletteRow}>
                      {palette.map((color, index) => (
                        <div key={`${color}-${index}`} style={{ ...styles.paletteChip, background: color }} />
                      ))}
                    </div>
                  ) : null}
                  {asset.tags ? (
                    <div style={styles.seriesTagBox}>
                      <div style={styles.seriesTagTitle}>Stable Diffusion Tags</div>
                      <div style={styles.seriesTagContent}>{asset.tags}</div>
                    </div>
                  ) : null}
                  <div style={styles.cardFooter}>
                    <Button onClick={() => alert('Add to Library logic here')}>入库</Button>
                  </div>
                </div>
              )
            }
            return (
              <div key={idx} style={styles.card}>
                <div style={styles.cardHeader}>
                  <div style={styles.cardTitle}>{asset.title}</div>
                </div>
                <div style={styles.cardContent}>{asset.content}</div>
                <div style={styles.cardFooter}>
                  <Button onClick={() => alert('Add to Library logic here')}>入库</Button>
                </div>
              </div>
            )
          })
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
  },
  characterCard: {
    width: 360,
    background: 'radial-gradient(120% 120% at 10% 0%, rgba(99,102,241,0.20), rgba(15,23,42,0.90))',
    border: '1px solid rgba(148,163,184,0.35)',
    borderRadius: 16,
    padding: 18,
    boxShadow: '0 22px 50px rgba(15,23,42,0.55), inset 0 1px 0 rgba(255,255,255,0.06)',
    display: 'flex',
    flexDirection: 'column',
    gap: 14,
    position: 'relative',
    overflow: 'hidden',
  },
  characterHeader: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: 12,
  },
  characterName: {
    fontSize: 18,
    fontWeight: 800,
    color: '#f8fafc',
    letterSpacing: 0.6,
    fontFamily: '"Noto Serif SC", "Songti SC", serif',
  },
  characterBadge: {
    padding: '4px 10px',
    borderRadius: 999,
    border: '1px solid rgba(148,163,184,0.45)',
    background: 'rgba(15,23,42,0.6)',
    fontSize: 11,
    letterSpacing: 1.2,
    textTransform: 'uppercase',
    color: '#cbd5f5',
    fontWeight: 700,
  },
  characterDesc: {
    fontSize: 12.5,
    lineHeight: 1.7,
    color: 'rgba(226,232,240,0.86)',
    fontFamily: '"Noto Sans SC", "PingFang SC", sans-serif',
  },
  characterDnaGrid: {
    display: 'grid',
    gridTemplateColumns: 'repeat(2, minmax(0, 1fr))',
    gap: 10,
  },
  dnaItem: {
    borderRadius: 12,
    padding: '10px 12px',
    background: 'rgba(15,23,42,0.55)',
    border: '1px solid rgba(99,102,241,0.28)',
    boxShadow: 'inset 0 1px 0 rgba(255,255,255,0.06)',
    display: 'flex',
    flexDirection: 'column',
    gap: 6,
  },
  dnaLabel: {
    fontSize: 10,
    textTransform: 'uppercase',
    letterSpacing: 1,
    color: 'rgba(199,210,254,0.8)',
    fontWeight: 700,
  },
  dnaValue: {
    fontSize: 11,
    lineHeight: 1.6,
    color: 'rgba(226,232,240,0.9)',
    whiteSpace: 'pre-wrap',
  },
  tagBlock: {
    borderRadius: 12,
    padding: '12px 14px',
    background: 'rgba(30,41,59,0.7)',
    border: '1px dashed rgba(148,163,184,0.45)',
    display: 'flex',
    flexDirection: 'column',
    gap: 6,
  },
  tagTitle: {
    fontSize: 10,
    textTransform: 'uppercase',
    letterSpacing: 1.2,
    color: 'rgba(226,232,240,0.7)',
    fontWeight: 700,
  },
  tagContent: {
    fontSize: 11.5,
    color: 'rgba(226,232,240,0.88)',
    lineHeight: 1.6,
    whiteSpace: 'pre-wrap',
  },
  propCard: {
    width: 320,
    padding: 18,
    borderRadius: 0,
    background: 'linear-gradient(135deg, rgba(15,23,42,0.92), rgba(59,7,100,0.75))',
    border: '1px solid rgba(236,72,153,0.45)',
    boxShadow: '0 18px 40px rgba(15,23,42,0.45)',
    display: 'flex',
    flexDirection: 'column',
    gap: 12,
    clipPath: 'polygon(6% 0%, 94% 0%, 100% 12%, 100% 100%, 0% 100%, 0% 12%)',
  },
  propHeader: {
    display: 'flex',
    alignItems: 'center',
    gap: 10,
  },
  propIcon: {
    padding: '4px 10px',
    borderRadius: 999,
    background: 'rgba(236,72,153,0.2)',
    border: '1px solid rgba(236,72,153,0.55)',
    fontSize: 11,
    color: '#fdf2f8',
    fontWeight: 700,
    letterSpacing: 1,
  },
  propName: {
    fontSize: 16,
    color: '#fff',
    fontWeight: 800,
    letterSpacing: 0.5,
  },
  propDesc: {
    fontSize: 12,
    lineHeight: 1.6,
    color: 'rgba(248,250,252,0.85)',
  },
  propTagBox: {
    padding: '10px 12px',
    borderRadius: 12,
    background: 'rgba(15,23,42,0.6)',
    border: '1px dashed rgba(236,72,153,0.5)',
    display: 'flex',
    flexDirection: 'column',
    gap: 6,
  },
  propTagTitle: {
    fontSize: 10,
    textTransform: 'uppercase',
    letterSpacing: 1.2,
    color: 'rgba(248,250,252,0.7)',
    fontWeight: 700,
  },
  propTagContent: {
    fontSize: 11,
    color: 'rgba(248,250,252,0.9)',
    lineHeight: 1.6,
    whiteSpace: 'pre-wrap',
  },
  locationCard: {
    width: 340,
    padding: 18,
    borderRadius: 20,
    background: 'linear-gradient(160deg, rgba(15,23,42,0.95), rgba(2,132,199,0.45))',
    border: '1px solid rgba(56,189,248,0.5)',
    boxShadow: '0 24px 46px rgba(3,7,18,0.6)',
    display: 'flex',
    flexDirection: 'column',
    gap: 12,
  },
  locationHeader: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: 10,
  },
  locationTitle: {
    fontSize: 16,
    fontWeight: 800,
    color: '#e0f2fe',
  },
  locationAccent: {
    width: 34,
    height: 34,
    borderRadius: 10,
    background: 'radial-gradient(circle, rgba(56,189,248,0.9), rgba(14,116,144,0.2))',
    border: '1px solid rgba(56,189,248,0.5)',
  },
  locationDesc: {
    fontSize: 12,
    lineHeight: 1.7,
    color: 'rgba(224,242,254,0.85)',
  },
  locationTagBox: {
    borderLeft: '3px solid rgba(56,189,248,0.7)',
    padding: '8px 12px',
    background: 'rgba(15,23,42,0.6)',
    borderRadius: 12,
    display: 'flex',
    flexDirection: 'column',
    gap: 6,
  },
  locationTagTitle: {
    fontSize: 10,
    textTransform: 'uppercase',
    letterSpacing: 1.2,
    color: 'rgba(186,230,253,0.7)',
    fontWeight: 700,
  },
  locationTagContent: {
    fontSize: 11,
    color: 'rgba(224,242,254,0.9)',
    lineHeight: 1.6,
    whiteSpace: 'pre-wrap',
  },
  seriesCard: {
    width: 420,
    padding: 20,
    borderRadius: 30,
    background: 'linear-gradient(180deg, rgba(15,23,42,0.95), rgba(88,28,135,0.75))',
    border: '1px solid rgba(167,139,250,0.55)',
    boxShadow: '0 26px 60px rgba(30,27,75,0.5)',
    display: 'flex',
    flexDirection: 'column',
    gap: 14,
  },
  seriesHeader: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: 10,
  },
  seriesTitle: {
    fontSize: 18,
    fontWeight: 800,
    color: '#f5f3ff',
  },
  seriesBadge: {
    padding: '6px 12px',
    borderRadius: 999,
    background: 'rgba(139,92,246,0.3)',
    border: '1px solid rgba(167,139,250,0.6)',
    fontSize: 11,
    letterSpacing: 1.1,
    textTransform: 'uppercase',
    color: '#ede9fe',
    fontWeight: 700,
  },
  seriesBody: {
    display: 'flex',
    flexDirection: 'column',
    gap: 10,
  },
  seriesRow: {
    display: 'flex',
    gap: 12,
  },
  seriesLabel: {
    minWidth: 90,
    fontSize: 10,
    textTransform: 'uppercase',
    letterSpacing: 1.2,
    color: 'rgba(233,213,255,0.7)',
    fontWeight: 700,
  },
  seriesValue: {
    fontSize: 12,
    lineHeight: 1.6,
    color: 'rgba(243,232,255,0.9)',
    flex: 1,
  },
  paletteRow: {
    display: 'flex',
    gap: 10,
    flexWrap: 'wrap',
  },
  paletteChip: {
    width: 26,
    height: 26,
    borderRadius: 8,
    border: '1px solid rgba(255,255,255,0.35)',
    boxShadow: '0 6px 12px rgba(15,23,42,0.4)',
  },
  seriesTagBox: {
    borderRadius: 16,
    padding: '12px 14px',
    background: 'rgba(15,23,42,0.6)',
    border: '1px dashed rgba(167,139,250,0.5)',
    display: 'flex',
    flexDirection: 'column',
    gap: 6,
  },
  seriesTagTitle: {
    fontSize: 10,
    textTransform: 'uppercase',
    letterSpacing: 1.2,
    color: 'rgba(233,213,255,0.75)',
    fontWeight: 700,
  },
  seriesTagContent: {
    fontSize: 11.5,
    lineHeight: 1.6,
    color: 'rgba(243,232,255,0.9)',
    whiteSpace: 'pre-wrap',
  },
}
