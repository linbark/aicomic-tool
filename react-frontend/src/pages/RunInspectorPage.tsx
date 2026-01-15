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

// [修改] 新增 StageItem 类型，包含 preview
type StageItem = {
  name: string
  preview?: string
  timestamp?: number
}

export function RunInspectorPage() {
  const { projects, projectId, setProjectId } = useProjectSelection()

  const [runs, setRuns] = useState<RunMeta[]>([])
  const [loadingRuns, setLoadingRuns] = useState(false)
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null)
  
  const [runData, setRunData] = useState<RunData | null>(null)
  const [loadingRun, setLoadingRun] = useState(false)
  
  // [修改] stages 状态改为对象数组
  const [stages, setStages] = useState<StageItem[]>([])
  const [loadingStages, setLoadingStages] = useState(false)
  const [selectedStage, setSelectedStage] = useState<string | null>(null)
  
  const [stageData, setStageData] = useState<unknown>(null)
  const [loadingStage, setLoadingStage] = useState(false)

  async function loadRuns() {
    if (!projectId) return
    console.log('[RunInspector] Loading runs for project:', projectId)
    setLoadingRuns(true)
    try {
      const res = await api.listRunFiles(projectId)
      console.log('[RunInspector] Runs loaded:', res.data)
      setRuns((res.data as any)?.runs || [])
    } catch (e: any) {
      console.error('[RunInspector] Failed to load runs:', e)
    } finally {
      setLoadingRuns(false)
    }
  }

  async function loadRun() {
    if (!projectId || !selectedRunId) return
    console.log('[RunInspector] Loading run details:', selectedRunId)
    setLoadingRun(true)
    try {
      const res = await api.getRunFile(projectId, selectedRunId)
      console.log('[RunInspector] Run details loaded:', res.data)
      setRunData(res.data as any)
    } catch (e: any) {
      console.error('[RunInspector] Failed to load run details:', e)
    } finally {
      setLoadingRun(false)
    }
  }

  async function loadStages() {
    if (!projectId || !selectedRunId) return
    console.log('[RunInspector] Loading stages for run:', selectedRunId)
    setLoadingStages(true)
    try {
      const res = await api.listRunStages(projectId, selectedRunId)
      console.log('[RunInspector] Stages loaded:', res.data)
      setStages(((res.data as any)?.stages || []) as StageItem[])
    } catch (e: any) {
      console.error('[RunInspector] Failed to load stages:', e)
    } finally {
      setLoadingStages(false)
    }
  }

  async function loadStage() {
    if (!projectId || !selectedRunId || !selectedStage) return
    console.log('[RunInspector] Loading stage content:', selectedStage)
    setLoadingStage(true)
    try {
      const res = await api.getRunStage(projectId, selectedRunId, selectedStage)
      console.log('[RunInspector] Stage content loaded:', res.data)
      setStageData((res.data as any)?.data)
    } catch (e: any) {
      console.error('[RunInspector] Failed to load stage content:', e)
    } finally {
      setLoadingStage(false)
    }
  }

  // 初始加载 Runs
  useEffect(() => {
    loadRuns().catch(() => {})
  }, [projectId])

  // 当选中 Run 变化时，加载 Run 详情和 Stages
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

  // 当选中 Stage 变化时，加载 Stage 内容
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
          <select value={projectId ?? ''} onChange={(e) => setProjectId(e.target.value || null)} style={styles.select}>
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
        {/* 左侧栏：Run 列表 */}
        <div style={styles.left}>
          <div style={styles.section}>
            <h3 style={{ margin: '0 0 12px 0', fontSize: 14 }}>Runs ({runs.length})</h3>
            {loadingRuns ? (
              <div style={styles.empty}>加载中…</div>
            ) : runs.length === 0 ? (
              <div style={styles.empty}>暂无 run 快照</div>
            ) : (
              <div style={styles.listContainer}>
                {runs.map((run) => (
                  <button
                    key={run.run_id}
                    onClick={() => setSelectedRunId(run.run_id)}
                    style={{
                      ...styles.listItem,
                      ...(selectedRunId === run.run_id ? styles.listItemActive : null),
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

        {/* 中间栏：Steps (Stages) 列表 */}
        <div style={styles.middle}>
           <h3 style={{ margin: '0 0 12px 0', fontSize: 14 }}>Execution Steps</h3>
           {!selectedRunId ? (
             <div style={styles.empty}>请选择 Run</div>
           ) : (
             <div style={styles.listContainer}>
               {/* Run Overview 选项 */}
               <button
                  style={{ 
                    ...styles.listItem, 
                    ...(!selectedStage ? styles.listItemActive : null),
                    borderLeft: !selectedStage ? '3px solid #6366f1' : '3px solid transparent'
                  }}
                  onClick={() => setSelectedStage(null)}
                >
                  <div style={{fontWeight: 600}}>Run Overview</div>
                  <div style={{fontSize: 10, opacity: 0.6}}>Meta & IO</div>
                </button>

                {/* Stage 列表 */}
                {loadingStages ? (
                   <div style={styles.empty}>加载 Steps…</div>
                ) : stages.length === 0 ? (
                   <div style={styles.empty}>无步骤记录</div>
                ) : (
                  stages.map(stage => (
                    <button
                      key={stage.name}
                      style={{ 
                        ...styles.listItem, 
                        ...(selectedStage === stage.name ? styles.listItemActive : null),
                        borderLeft: selectedStage === stage.name ? '3px solid #6366f1' : '3px solid transparent'
                      }}
                      onClick={() => setSelectedStage(stage.name)}
                    >
                      <div style={{fontSize: 12, fontWeight: 600}}>{stage.name}</div>
                      {/* [修改] 显示预览文本 */}
                      {stage.preview ? (
                        <div style={{
                          fontSize: 10, 
                          opacity: 0.7, 
                          marginTop: 4, 
                          // 使用 CSS line-clamp 实现多行截断
                          display: '-webkit-box', 
                          WebkitLineClamp: 3, 
                          WebkitBoxOrient: 'vertical', 
                          overflow: 'hidden',
                          whiteSpace: 'pre-wrap', 
                          textAlign: 'left',
                          lineHeight: '1.4'
                        }}>
                          {stage.preview}
                        </div>
                      ) : null}
                    </button>
                  ))
                )}
             </div>
           )}
        </div>

        {/* 右侧栏：详情内容 */}
        <div style={styles.right}>
          {!selectedRunId ? (
            <div style={styles.empty}>请从左侧选择一个 run</div>
          ) : (
            <div style={styles.content}>
              <div style={styles.panel}>
                {selectedStage ? (
                  // 显示 Stage 详情
                  <div style={{height: '100%', display: 'flex', flexDirection: 'column'}}>
                    <div style={styles.panelHeader}>
                      <div style={{ fontWeight: 700 }}>Step: {selectedStage}</div>
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
                  // 显示 Run Overview
                  <div style={{height: '100%', display: 'flex', flexDirection: 'column'}}>
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
                        <div style={{ flex: 1, minHeight: 0, overflow: 'auto' }}>
                            <h4 style={{ margin: '0 0 8px 0', fontSize: 13, color: '#818cf8' }}>Meta</h4>
                            <pre style={{...styles.json, maxHeight: 150}}>{JSON.stringify(runData.meta, null, 2)}</pre>
                            
                            <h4 style={{ margin: '16px 0 8px 0', fontSize: 13, color: '#34d399' }}>Request</h4>
                            <pre style={{...styles.json, maxHeight: 200}}>{JSON.stringify(runData.request, null, 2)}</pre>
                            
                            <h4 style={{ margin: '16px 0 8px 0', fontSize: 13, color: '#f472b6' }}>Response</h4>
                            <pre style={styles.json}>{JSON.stringify(runData.response, null, 2)}</pre>
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
  body: { display: 'grid', gridTemplateColumns: '250px 250px 1fr', gap: 12, padding: 14, overflow: 'hidden', flex: 1 },
  
  left: {
    border: '1px solid rgba(255,255,255,0.08)',
    borderRadius: 12,
    background: 'rgba(255,255,255,0.03)',
    padding: 12,
    overflow: 'hidden',
    display: 'flex',
    flexDirection: 'column',
  },
  middle: {
    border: '1px solid rgba(255,255,255,0.08)',
    borderRadius: 12,
    background: 'rgba(255,255,255,0.03)',
    padding: 12,
    overflow: 'hidden',
    display: 'flex',
    flexDirection: 'column',
  },
  right: {
    border: '1px solid rgba(255,255,255,0.08)',
    borderRadius: 12,
    background: 'rgba(255,255,255,0.03)',
    padding: 12,
    overflow: 'hidden',
    display: 'flex',
    flexDirection: 'column',
  },

  section: { display: 'flex', flexDirection: 'column', height: '100%' },
  
  listContainer: { display: 'flex', flexDirection: 'column', gap: 8, overflowY: 'auto', flex: 1, paddingRight: 4 },
  listItem: {
    textAlign: 'left',
    borderRadius: 8,
    border: '1px solid rgba(255,255,255,0.05)',
    background: 'rgba(0,0,0,0.18)',
    color: '#e5e7eb',
    padding: '10px 12px',
    cursor: 'pointer',
    display: 'flex',
    flexDirection: 'column',
    gap: 4,
    transition: 'all 0.2s',
  },
  listItemActive: {
    border: '1px solid rgba(99,102,241,0.6)',
    background: 'rgba(99,102,241,0.15)',
  },

  content: { display: 'flex', flexDirection: 'column', gap: 12, flex: 1, overflow: 'hidden', height: '100%' },
  
  panel: {
    flex: 1,
    overflow: 'hidden',
    display: 'flex',
    flexDirection: 'column',
  },
  panelHeader: {
    display: 'flex',
    justifyContent: 'space-between',
    alignItems: 'center',
    marginBottom: 12,
    paddingBottom: 8,
    borderBottom: '1px solid rgba(255,255,255,0.1)',
  },
  runDetails: { display: 'flex', flexDirection: 'column', height: '100%', overflow: 'hidden' },
  json: {
    background: 'rgba(0,0,0,0.3)',
    padding: 12,
    borderRadius: 8,
    overflow: 'auto',
    fontSize: 12,
    fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace',
    lineHeight: 1.6,
    color: '#e5e7eb',
    margin: 0,
    flex: 1, 
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
    borderRadius: 6,
    border: '1px solid rgba(255,255,255,0.14)',
    background: 'rgba(255,255,255,0.08)',
    color: '#e5e7eb',
    padding: '4px 12px',
    cursor: 'pointer',
    fontSize: 12,
  },
  empty: { opacity: 0.6, fontSize: 13, padding: 20, textAlign: 'center', marginTop: 20 },
}
