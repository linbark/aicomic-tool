import { useEffect, useState } from 'react'
import api from '../api/client'
import { useProjectSelection } from '../state/useProjectSelection'

type RunMeta = {
  project_id: string
  run_id: string
  created_at_ms: number
  workflow?: string
}

type RunData = {
  request: Record<string, unknown>
  response: Record<string, unknown>
  meta: Record<string, unknown>
}

export function RunInspectorPage() {
  const { projects, projectId, setProjectId } = useProjectSelection()

  const [runs, setRuns] = useState<RunMeta[]>([])
  const [loadingRuns, setLoadingRuns] = useState(false)
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null)
  const [runData, setRunData] = useState<RunData | null>(null)
  const [loadingRun, setLoadingRun] = useState(false)
  const [stages, setStages] = useState<string[]>([])
  const [loadingStages, setLoadingStages] = useState(false)
  const [selectedStage, setSelectedStage] = useState<string | null>(null)
  const [stageData, setStageData] = useState<unknown>(null)
  const [loadingStage, setLoadingStage] = useState(false)

  async function loadRuns() {
    if (!projectId) return
    setLoadingRuns(true)
    try {
      const res = await api.listRunFiles(projectId)
      setRuns((res.data as any)?.runs || [])
    } catch (e: any) {
      console.error('加载 runs 失败', e)
    } finally {
      setLoadingRuns(false)
    }
  }

  async function loadRun() {
    if (!projectId || !selectedRunId) return
    setLoadingRun(true)
    try {
      const res = await api.getRunFile(projectId, selectedRunId)
      setRunData(res.data as any)
    } catch (e: any) {
      console.error('加载 run 失败', e)
    } finally {
      setLoadingRun(false)
    }
  }

  async function loadStages() {
    if (!projectId || !selectedRunId) return
    setLoadingStages(true)
    try {
      const res = await api.listRunStages(projectId, selectedRunId)
      setStages((res.data as any)?.stages || [])
    } catch (e: any) {
      console.error('加载 stages 失败', e)
    } finally {
      setLoadingStages(false)
    }
  }

  async function loadStage() {
    if (!projectId || !selectedRunId || !selectedStage) return
    setLoadingStage(true)
    try {
      const res = await api.getRunStage(projectId, selectedRunId, selectedStage)
      setStageData((res.data as any)?.data)
    } catch (e: any) {
      console.error('加载 stage 失败', e)
    } finally {
      setLoadingStage(false)
    }
  }

  useEffect(() => {
    loadRuns().catch(() => {})
  }, [projectId])

  useEffect(() => {
    if (selectedRunId) {
      loadRun().catch(() => {})
      loadStages().catch(() => {})
    } else {
      setRunData(null)
      setStages([])
      setSelectedStage(null)
      setStageData(null)
    }
  }, [projectId, selectedRunId])

  useEffect(() => {
    if (selectedStage) {
      loadStage().catch(() => {})
    } else {
      setStageData(null)
    }
  }, [projectId, selectedRunId, selectedStage])

  const formatTime = (ms: number) => {
    return new Date(ms).toLocaleString('zh-CN')
  }

  return (
    <div style={styles.page}>
      <div style={styles.header}>
        <div>
          <h2 style={{ margin: 0, fontSize: 20 }}>Run Inspector</h2>
          <div style={{ fontSize: 12, opacity: 0.7 }}>Workflow 快照审计与回放</div>
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
          <button style={styles.btn} onClick={() => loadRuns().catch(() => {})} disabled={loadingRuns || !projectId}>
            {loadingRuns ? '加载中…' : '刷新'}
          </button>
        </div>
      </div>

      <div style={styles.body}>
        <div style={styles.left}>
          <div style={styles.section}>
            <h3 style={{ margin: '0 0 12px 0', fontSize: 14 }}>Runs ({runs.length})</h3>
            {loadingRuns ? (
              <div style={styles.empty}>加载中…</div>
            ) : runs.length === 0 ? (
              <div style={styles.empty}>暂无 run 快照</div>
            ) : (
              <div style={styles.runList}>
                {runs.map((run) => (
                  <button
                    key={run.run_id}
                    onClick={() => setSelectedRunId(run.run_id)}
                    style={{
                      ...styles.runItem,
                      ...(selectedRunId === run.run_id ? styles.runItemActive : null),
                    }}
                  >
                    <div style={{ fontSize: 11, fontFamily: 'monospace', opacity: 0.8 }}>{run.run_id.slice(0, 8)}...</div>
                    <div style={{ fontSize: 11, opacity: 0.7 }}>{formatTime(run.created_at_ms)}</div>
                    {run.workflow ? <div style={{ fontSize: 10, opacity: 0.6 }}>{run.workflow}</div> : null}
                  </button>
                ))}
              </div>
            )}
          </div>
        </div>

        <div style={styles.right}>
          {!selectedRunId ? (
            <div style={styles.empty}>请从左侧选择一个 run</div>
          ) : (
            <div style={styles.content}>
              <div style={styles.tabs}>
                <button
                  style={{ ...styles.tab, ...(!selectedStage ? styles.tabActive : null) }}
                  onClick={() => setSelectedStage(null)}
                  disabled={loadingStages}
                >
                  Run 详情
                </button>
                {stages.map((stage) => (
                  <button
                    key={stage}
                    style={{ ...styles.tab, ...(selectedStage === stage ? styles.tabActive : null) }}
                    onClick={() => setSelectedStage(stage)}
                    disabled={loadingStages}
                  >
                    {stage}
                  </button>
                ))}
                {loadingStages ? <div style={{ marginLeft: 10, fontSize: 11, opacity: 0.7 }}>Stages 加载中…</div> : null}
              </div>

              <div style={styles.panel}>
                {selectedStage ? (
                  <div>
                    <div style={styles.panelHeader}>
                      <div style={{ fontWeight: 700 }}>Stage: {selectedStage}</div>
                      <button style={styles.btn} onClick={() => loadStage().catch(() => {})} disabled={loadingStage}>
                        {loadingStage ? '加载中…' : '刷新'}
                      </button>
                    </div>
                    {loadingStage ? (
                      <div style={styles.empty}>加载中…</div>
                    ) : (
                      <pre style={styles.json}>{JSON.stringify(stageData, null, 2)}</pre>
                    )}
                  </div>
                ) : (
                  <div>
                    <div style={styles.panelHeader}>
                      <div style={{ fontWeight: 700 }}>Run ID: {selectedRunId}</div>
                      <button style={styles.btn} onClick={() => loadRun().catch(() => {})} disabled={loadingRun}>
                        {loadingRun ? '加载中…' : '刷新'}
                      </button>
                    </div>
                    {loadingRun ? (
                      <div style={styles.empty}>加载中…</div>
                    ) : runData ? (
                      <div style={styles.runDetails}>
                        <div>
                          <h4 style={{ margin: '0 0 8px 0', fontSize: 13 }}>Request</h4>
                          <pre style={styles.json}>{JSON.stringify(runData.request, null, 2)}</pre>
                        </div>
                        <div style={{ marginTop: 20 }}>
                          <h4 style={{ margin: '0 0 8px 0', fontSize: 13 }}>Response</h4>
                          <pre style={styles.json}>{JSON.stringify(runData.response, null, 2)}</pre>
                        </div>
                        <div style={{ marginTop: 20 }}>
                          <h4 style={{ margin: '0 0 8px 0', fontSize: 13 }}>Meta</h4>
                          <pre style={styles.json}>{JSON.stringify(runData.meta, null, 2)}</pre>
                        </div>
                      </div>
                    ) : (
                      <div style={styles.empty}>加载失败</div>
                    )}
                  </div>
                )}
              </div>
            </div>
          )}
        </div>
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
  body: { display: 'grid', gridTemplateColumns: '280px 1fr', gap: 12, padding: 14, overflow: 'hidden', flex: 1 },
  left: {
    border: '1px solid rgba(255,255,255,0.08)',
    borderRadius: 12,
    background: 'rgba(255,255,255,0.03)',
    padding: 12,
    overflow: 'auto',
  },
  right: {
    border: '1px solid rgba(255,255,255,0.08)',
    borderRadius: 12,
    background: 'rgba(255,255,255,0.03)',
    padding: 12,
    overflow: 'auto',
    display: 'flex',
    flexDirection: 'column',
  },
  section: { marginBottom: 20 },
  runList: { display: 'flex', flexDirection: 'column', gap: 8 },
  runItem: {
    textAlign: 'left',
    borderRadius: 10,
    border: '1px solid rgba(255,255,255,0.10)',
    background: 'rgba(0,0,0,0.18)',
    color: '#e5e7eb',
    padding: 10,
    cursor: 'pointer',
    display: 'flex',
    flexDirection: 'column',
    gap: 4,
  },
  runItemActive: {
    border: '1px solid rgba(99,102,241,0.8)',
    background: 'rgba(99,102,241,0.12)',
  },
  content: { display: 'flex', flexDirection: 'column', gap: 12, flex: 1, overflow: 'hidden' },
  tabs: { display: 'flex', gap: 4, borderBottom: '1px solid rgba(255,255,255,0.1)', overflowX: 'auto' },
  tab: {
    padding: '8px 16px',
    cursor: 'pointer',
    background: 'transparent',
    border: 'none',
    borderBottom: '2px solid transparent',
    color: '#94a3b8',
    fontSize: 12,
    fontWeight: 600,
    whiteSpace: 'nowrap',
  },
  tabActive: {
    color: '#6366f1',
    borderBottom: '2px solid #6366f1',
  },
  panel: {
    flex: 1,
    overflow: 'auto',
    border: '1px solid rgba(255,255,255,0.08)',
    borderRadius: 12,
    background: 'rgba(0,0,0,0.18)',
    padding: 16,
  },
  panelHeader: {
    display: 'flex',
    justifyContent: 'space-between',
    alignItems: 'center',
    marginBottom: 12,
  },
  runDetails: { display: 'flex', flexDirection: 'column' },
  json: {
    background: 'rgba(0,0,0,0.3)',
    padding: 12,
    borderRadius: 8,
    overflow: 'auto',
    fontSize: 12,
    fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace',
    lineHeight: 1.6,
    color: '#e5e7eb',
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
    fontSize: 12,
  },
  empty: { opacity: 0.6, fontSize: 13, padding: 20, textAlign: 'center' },
}

