import { memo, useState } from 'react'
import { SharedStepLayout } from './SharedStepLayout'
import { Button } from '../../ui/Button'
import type { EpisodeRead } from '../../../api/types'

type Step3Props = {
  episode: EpisodeRead
  onRun: () => void
  busy: boolean
}

export const Step3_Storyboard = memo(function Step3_Storyboard({
  episode,
  onRun,
  busy,
}: Step3Props) {
  const [showCode, setShowCode] = useState(true)

  const toolbar = (
    <>
      <Button onClick={() => setShowCode(!showCode)}>
        {showCode ? '隐藏代码' : '显示代码'}
      </Button>
      <Button variant="primary" onClick={onRun} disabled={busy}>
        {busy ? '生成分镜...' : '生成分镜'}
      </Button>
    </>
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
        Storyboard JSON
      </div>
      <textarea
        value={JSON.stringify(episode.scenes || [], null, 2)}
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
    >
      <div style={{ padding: 40, minHeight: '100%', display: 'flex', flexDirection: 'column', gap: 40 }}>
        {(episode.scenes || []).length > 0 ? (
          (episode.scenes || []).map((scene) => (
            <div key={scene.id} style={styles.sceneGroup}>
              <div style={styles.sceneHeader}>
                <span style={styles.sceneTitle}>SC{scene.sequence_number} - {scene.title}</span>
                <span style={styles.sceneDesc}>{scene.description}</span>
              </div>
              
              <div style={styles.shotGrid}>
                {(scene.shots || []).map((shot) => (
                  <div key={shot.id} style={styles.shotCard}>
                    <div style={styles.shotHeader}>
                      <span style={styles.shotNum}>SH{shot.sequence_number}</span>
                      <span style={styles.shotTitle}>{shot.title}</span>
                    </div>
                    <div style={styles.shotContent}>
                      {shot.action_text || '无动作描述'}
                    </div>
                    <div style={styles.shotFooter}>
                      <div style={styles.tag}>{shot.status || '—'}</div>
                      <div style={styles.tag}>{shot.prompt ? 'Prompt' : 'No prompt'}</div>
                    </div>
                  </div>
                ))}
                {(scene.shots || []).length === 0 && (
                   <div style={styles.emptyShot}>暂无镜头</div>
                )}
              </div>
            </div>
          ))
        ) : (
          <div style={{ opacity: 0.5, textAlign: 'center', marginTop: 100 }}>暂无场景数据，请点击生成...</div>
        )}
      </div>
    </SharedStepLayout>
  )
})

const styles: Record<string, React.CSSProperties> = {
  sceneGroup: {
    display: 'flex',
    flexDirection: 'column',
    gap: 16,
  },
  sceneHeader: {
    display: 'flex',
    alignItems: 'baseline',
    gap: 12,
    borderBottom: '1px solid rgba(255,255,255,0.1)',
    paddingBottom: 8,
  },
  sceneTitle: {
    fontSize: 16,
    fontWeight: 700,
    color: '#fff',
    fontFamily: 'monospace',
  },
  sceneDesc: {
    fontSize: 13,
    opacity: 0.6,
  },
  shotGrid: {
    display: 'flex',
    flexWrap: 'wrap',
    gap: 16,
  },
  shotCard: {
    width: 200,
    background: 'rgba(255,255,255,0.05)',
    border: '1px solid rgba(255,255,255,0.1)',
    borderRadius: 8,
    padding: 12,
    display: 'flex',
    flexDirection: 'column',
    gap: 8,
    cursor: 'grab',
  },
  shotHeader: {
    display: 'flex',
    justifyContent: 'space-between',
    fontSize: 12,
    fontWeight: 700,
    color: 'rgba(255,255,255,0.9)',
  },
  shotNum: {
    fontFamily: 'monospace',
    opacity: 0.7,
  },
  shotTitle: {
    opacity: 0.9,
  },
  shotContent: {
    fontSize: 11,
    opacity: 0.7,
    lineHeight: 1.4,
    flex: 1,
    minHeight: 40,
  },
  shotFooter: {
    display: 'flex',
    gap: 6,
  },
  tag: {
    fontSize: 9,
    background: 'rgba(255,255,255,0.1)',
    padding: '2px 6px',
    borderRadius: 4,
    opacity: 0.8,
  },
  emptyShot: {
    fontSize: 12,
    opacity: 0.4,
    fontStyle: 'italic',
    padding: 10,
  }
}
