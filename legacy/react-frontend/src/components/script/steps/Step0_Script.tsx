import { memo } from 'react'
import { SharedStepLayout } from './SharedStepLayout'
import { Button } from '../../ui/Button'

type Step0Props = {
  text: string
  onChange: (text: string) => void
  onNext: () => void
  busy: boolean
}

export const Step0_Script = memo(function Step0_Script({
  text,
  onChange,
  onNext,
  busy,
}: Step0Props) {
  
  const toolbar = (
    <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
      <Button variant="primary" onClick={onNext} disabled={busy}>
        {busy ? '执行中...' : '结构拆解 →'}
      </Button>
    </div>
  )

  return (
    <SharedStepLayout toolbar={toolbar}>
      <div style={{ padding: 20, height: '100%', boxSizing: 'border-box' }}>
        <textarea
          value={text}
          onChange={(e) => onChange(e.target.value)}
          placeholder="在此输入原始剧本..."
          style={{
            width: '100%',
            height: '100%',
            background: 'transparent',
            border: 'none',
            color: '#e5e7eb',
            fontSize: 14,
            lineHeight: 1.6,
            fontFamily: 'Menlo, Monaco, monospace',
            resize: 'none',
            outline: 'none',
          }}
        />
      </div>
    </SharedStepLayout>
  )
})
