import { useEffect, useMemo, useState } from 'react'
import api from '../api/client'
import type { AssetItemRead } from '../api/types'
import { useProjectSelection } from '../state/useProjectSelection'

export function ContextPage() {
  const { projects, projectId, setProjectId } = useProjectSelection()

  const [seriesBibleJson, setSeriesBibleJson] = useState('')
  const [seriesBibleError, setSeriesBibleError] = useState<string | null>(null)
  const [savingSeriesBible, setSavingSeriesBible] = useState(false)
  const [loadingSeriesBible, setLoadingSeriesBible] = useState(false)

  const [selectedItemId, setSelectedItemId] = useState<number | null>(null)
  const [assetItems, setAssetItems] = useState<AssetItemRead[]>([])
  const [loadingItems, setLoadingItems] = useState(false)
  const [visualDnaJson, setVisualDnaJson] = useState('')
  const [visualDnaError, setVisualDnaError] = useState<string | null>(null)
  const [savingVisualDna, setSavingVisualDna] = useState(false)
  const [loadingVisualDna, setLoadingVisualDna] = useState(false)

  async function loadSeriesBible() {
    if (!projectId) return
    setLoadingSeriesBible(true)
    setSeriesBibleError(null)
    try {
      const res = await api.getSeriesBible(projectId, 'v1')
      const data = res.data
      if (data?.exists && data.data) {
        setSeriesBibleJson(JSON.stringify(data.data, null, 2))
      } else {
        setSeriesBibleJson('{}')
      }
    } catch (e: any) {
      setSeriesBibleError(e?.response?.data?.detail || e?.message || '加载失败')
    } finally {
      setLoadingSeriesBible(false)
    }
  }

  async function saveSeriesBible() {
    if (!projectId) return
    let parsed: Record<string, unknown>
    try {
      parsed = JSON.parse(seriesBibleJson)
    } catch (e) {
      setSeriesBibleError('JSON 格式无效')
      return
    }
    setSavingSeriesBible(true)
    setSeriesBibleError(null)
    try {
      await api.putSeriesBible(projectId, { data: parsed, version: 'v1' })
      setSeriesBibleError(null)
    } catch (e: any) {
      setSeriesBibleError(e?.response?.data?.detail || e?.message || '保存失败')
    } finally {
      setSavingSeriesBible(false)
    }
  }

  async function loadAssetItems() {
    if (!projectId) return
    setLoadingItems(true)
    try {
      const res = await api.getAssetItems(projectId)
      setAssetItems(res.data || [])
      if (res.data && res.data.length > 0 && !selectedItemId) {
        setSelectedItemId(res.data[0].id)
      }
    } catch (e: any) {
      console.error('加载资产条目失败', e)
    } finally {
      setLoadingItems(false)
    }
  }

  async function loadVisualDna() {
    if (!projectId || !selectedItemId) return
    setLoadingVisualDna(true)
    setVisualDnaError(null)
    try {
      const res = await api.getVisualDna(projectId, selectedItemId, 'v1')
      const data = res.data
      if (data?.exists && data.data) {
        setVisualDnaJson(JSON.stringify(data.data, null, 2))
      } else {
        setVisualDnaJson('{}')
      }
    } catch (e: any) {
      setVisualDnaError(e?.response?.data?.detail || e?.message || '加载失败')
    } finally {
      setLoadingVisualDna(false)
    }
  }

  async function saveVisualDna() {
    if (!projectId || !selectedItemId) return
    let parsed: Record<string, unknown>
    try {
      parsed = JSON.parse(visualDnaJson)
    } catch (e) {
      setVisualDnaError('JSON 格式无效')
      return
    }
    setSavingVisualDna(true)
    setVisualDnaError(null)
    try {
      await api.putVisualDna(projectId, selectedItemId, { data: parsed, version: 'v1' })
      setVisualDnaError(null)
    } catch (e: any) {
      setVisualDnaError(e?.response?.data?.detail || e?.message || '保存失败')
    } finally {
      setSavingVisualDna(false)
    }
  }

  useEffect(() => {
    loadSeriesBible().catch(() => {})
  }, [projectId])

  useEffect(() => {
    loadAssetItems().catch(() => {})
  }, [projectId])

  useEffect(() => {
    if (selectedItemId) {
      loadVisualDna().catch(() => {})
    } else {
      setVisualDnaJson('')
    }
  }, [projectId, selectedItemId])

  const selectedItem = useMemo(() => {
    return assetItems.find((item) => item.id === selectedItemId) || null
  }, [assetItems, selectedItemId])

  return (
    <div style={styles.page}>
      <div style={styles.header}>
        <div>
          <h2 style={{ margin: 0, fontSize: 20 }}>Context 管理</h2>
          <div style={{ fontSize: 12, opacity: 0.7 }}>Series Bible & Visual DNA（文件优先存储）</div>
        </div>
        <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
          <select value={projectId ?? ''} onChange={(e) => setProjectId(e.target.value ? Number(e.target.value) : null)} style={styles.select}>
            <option value="" disabled>
              选择项目…
            </option>
            {projects.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </select>
        </div>
      </div>

      <div style={styles.body}>
        {/* Series Bible */}
        <section style={styles.section}>
          <div style={styles.sectionHeader}>
            <div>
              <h3 style={{ margin: 0, fontSize: 16 }}>Series Bible</h3>
              <div style={{ fontSize: 11, opacity: 0.7 }}>项目级世界观设定（JSON）</div>
            </div>
            <div style={{ display: 'flex', gap: 8 }}>
              <button style={styles.btn} onClick={() => loadSeriesBible().catch(() => {})} disabled={loadingSeriesBible || !projectId}>
                {loadingSeriesBible ? '加载中…' : '重新加载'}
              </button>
              <button style={styles.btnPrimary} onClick={() => saveSeriesBible().catch(() => {})} disabled={savingSeriesBible || !projectId}>
                {savingSeriesBible ? '保存中…' : '保存'}
              </button>
            </div>
          </div>
          {seriesBibleError ? <div style={styles.error}>{seriesBibleError}</div> : null}
          <textarea
            value={seriesBibleJson}
            onChange={(e) => {
              setSeriesBibleJson(e.target.value)
              setSeriesBibleError(null)
            }}
            style={styles.jsonEditor}
            placeholder="Series Bible JSON..."
            disabled={!projectId}
          />
        </section>

        {/* Visual DNA */}
        <section style={styles.section}>
          <div style={styles.sectionHeader}>
            <div>
              <h3 style={{ margin: 0, fontSize: 16 }}>Visual DNA</h3>
              <div style={{ fontSize: 11, opacity: 0.7 }}>资产条目级视觉 DNA（JSON）</div>
            </div>
            <div style={{ display: 'flex', gap: 8 }}>
              <select
                value={selectedItemId ?? ''}
                onChange={(e) => setSelectedItemId(e.target.value ? Number(e.target.value) : null)}
                style={styles.select}
                disabled={!projectId || loadingItems}
              >
                <option value="">选择资产条目…</option>
                {assetItems.map((item) => (
                  <option key={item.id} value={item.id}>
                    {item.name} ({item.category})
                  </option>
                ))}
              </select>
              <button style={styles.btn} onClick={() => loadVisualDna().catch(() => {})} disabled={loadingVisualDna || !projectId || !selectedItemId}>
                {loadingVisualDna ? '加载中…' : '重新加载'}
              </button>
              <button style={styles.btnPrimary} onClick={() => saveVisualDna().catch(() => {})} disabled={savingVisualDna || !projectId || !selectedItemId}>
                {savingVisualDna ? '保存中…' : '保存'}
              </button>
            </div>
          </div>
          {selectedItem ? (
            <div style={{ fontSize: 12, opacity: 0.7, marginBottom: 8, display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
              <span>
                当前条目：{selectedItem.name} (ID: {selectedItem.id})
              </span>
              {selectedItem.assets && selectedItem.assets.length > 0 ? (
                <select
                  onChange={async (e) => {
                    const filePath = e.target.value
                    if (!filePath || !projectId || !selectedItemId) return
                    try {
                      const res = await api.ingestVisualDna({
                        project_id: projectId,
                        item_id: selectedItemId,
                        asset_file_path: filePath,
                        version: 'v1',
                      })
                      if (res.data?.visual_dna) {
                        setVisualDnaJson(JSON.stringify(res.data.visual_dna, null, 2))
                        setVisualDnaError(null)
                        await loadVisualDna()
                      }
                    } catch (err: any) {
                      setVisualDnaError(err?.response?.data?.detail || err?.message || '摄取失败')
                    }
                  }}
                  style={{ ...styles.select, fontSize: 11, padding: '4px 8px' }}
                >
                  <option value="">从图片摄取 Visual DNA…</option>
                  {selectedItem.assets
                    .filter((a) => a.file_type === 'image')
                    .map((a) => (
                      <option key={a.id} value={a.file_path}>
                        {a.file_path.split('/').pop()}
                      </option>
                    ))}
                </select>
              ) : null}
            </div>
          ) : null}
          {visualDnaError ? <div style={styles.error}>{visualDnaError}</div> : null}
          <textarea
            value={visualDnaJson}
            onChange={(e) => {
              setVisualDnaJson(e.target.value)
              setVisualDnaError(null)
            }}
            style={styles.jsonEditor}
            placeholder="Visual DNA JSON..."
            disabled={!projectId || !selectedItemId}
          />
        </section>
      </div>
    </div>
  )
}

const styles: Record<string, React.CSSProperties> = {
  page: { display: 'flex', flexDirection: 'column', gap: 12, height: 'calc(100vh - 32px)' },
  header: {
    padding: 14,
    borderRadius: 12,
    border: '1px solid rgba(255,255,255,0.08)',
    background: 'rgba(255,255,255,0.04)',
    display: 'flex',
    justifyContent: 'space-between',
    alignItems: 'center',
    gap: 12,
    flexWrap: 'wrap',
  },
  body: { padding: 14, overflow: 'auto', display: 'flex', flexDirection: 'column', gap: 20 },
  section: {
    border: '1px solid rgba(255,255,255,0.08)',
    borderRadius: 12,
    background: 'rgba(255,255,255,0.03)',
    padding: 16,
  },
  sectionHeader: {
    display: 'flex',
    justifyContent: 'space-between',
    alignItems: 'center',
    gap: 12,
    marginBottom: 12,
    flexWrap: 'wrap',
  },
  jsonEditor: {
    width: '100%',
    height: 400,
    resize: 'vertical',
    borderRadius: 12,
    border: '1px solid rgba(255,255,255,0.10)',
    background: 'rgba(0,0,0,0.18)',
    color: '#e5e7eb',
    padding: 12,
    boxSizing: 'border-box',
    outline: 'none',
    fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace',
    fontSize: 13,
    lineHeight: 1.6,
  },
  select: {
    borderRadius: 10,
    border: '1px solid rgba(255,255,255,0.10)',
    background: 'rgba(0,0,0,0.18)',
    color: '#e5e7eb',
    padding: '8px 10px',
    outline: 'none',
  },
  btn: {
    borderRadius: 10,
    border: '1px solid rgba(255,255,255,0.14)',
    background: 'rgba(255,255,255,0.04)',
    color: '#e5e7eb',
    padding: '8px 10px',
    cursor: 'pointer',
  },
  btnPrimary: {
    borderRadius: 10,
    border: '1px solid rgba(99,102,241,0.6)',
    background: 'rgba(99,102,241,0.35)',
    color: '#fff',
    padding: '8px 10px',
    cursor: 'pointer',
  },
  error: { color: '#f87171', fontSize: 12, marginBottom: 8 },
}

