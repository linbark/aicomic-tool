import { memo, useMemo, useRef } from 'react'
import type { EpisodeRead } from '../../api/types'
import type { ChatRunUi } from './types'
import { Button } from '../ui/Button'
import { Textarea } from '../ui/Textarea'
import { RunStatusPanel } from './RunStatusPanel'

type ExecutionPanelProps = {
  episode: EpisodeRead
  execRun: ChatRunUi | null
  execPollPaused: boolean
  uiNowMs: number
  interruptKind: string | null
  execBusy: boolean
  disableExecute?: boolean
  rawAssetsVisualDnaText?: string
  rawSplitEpisodesText?: string
  onExecute: () => void
  onPausePoll: () => void
  onResumePoll: () => void
  onForceRefresh: () => void
  onConfirm: (decision: 'confirmed' | 'regenerate' | 'rejected', artifacts?: Record<string, unknown>) => void
}

export const ExecutionPanel = memo(function ExecutionPanel({
  episode,
  execRun,
  execPollPaused,
  uiNowMs,
  interruptKind,
  execBusy,
  disableExecute,
  rawAssetsVisualDnaText,
  rawSplitEpisodesText,
  onExecute,
  onPausePoll,
  onResumePoll,
  onForceRefresh,
  onConfirm,
}: ExecutionPanelProps) {
  const artifacts = (episode.exec_artifacts || {}) as Record<string, unknown>
  const outline = typeof artifacts.outline === 'string' ? artifacts.outline : ''
  const assetsVisualDna = artifacts.assets_visual_dna
  const splitEpisodes = artifacts.split_episodes
  const ingestPreview = artifacts.ingest_preview

  const effectiveInterruptKind = useMemo(() => {
    if (interruptKind) return interruptKind
    const st = String(episode.exec_status || '')
    if (st === 'waiting_outline_confirm') return 'confirm_outline'
    if (st === 'waiting_assets_confirm') return 'confirm_assets'
    if (st === 'waiting_split_confirm') return 'confirm_split'
    if (st === 'waiting_ingest_confirm') return 'confirm_ingest'
    return null
  }, [episode.exec_status, interruptKind])

  const assetsTextareaRef = useRef<HTMLTextAreaElement>(null)
  const splitTextareaRef = useRef<HTMLTextAreaElement>(null)

  const assetsText = useMemo(() => {
    if (assetsVisualDna) return JSON.stringify(assetsVisualDna, null, 2)
    return String(rawAssetsVisualDnaText || '').trim()
  }, [assetsVisualDna, rawAssetsVisualDnaText])

  const splitText = useMemo(() => {
    if (splitEpisodes) return JSON.stringify(splitEpisodes, null, 2)
    return String(rawSplitEpisodesText || '').trim()
  }, [rawSplitEpisodesText, splitEpisodes])

  const confirmControls = useMemo(() => {
    if (!effectiveInterruptKind) return null
    if (effectiveInterruptKind === 'confirm_outline') {
      return (
        <div style={{ display: 'flex', gap: 8 }}>
          <Button variant="primary" disabled={execBusy} onClick={() => onConfirm('confirmed')}>
            确认继续
          </Button>
          <Button disabled={execBusy} onClick={() => onConfirm('regenerate')}>
            重新生成大纲
          </Button>
        </div>
      )
    }
    if (effectiveInterruptKind === 'confirm_assets') {
      return (
        <div style={{ display: 'flex', gap: 8 }}>
          <Button
            variant="primary"
            disabled={execBusy}
            onClick={() => {
              try {
                const text = assetsTextareaRef.current?.value ?? assetsText
                const parsed = text.trim() ? JSON.parse(text) : null
                onConfirm('confirmed', parsed ? { assets_visual_dna: parsed } : undefined)
              } catch {
                onConfirm('confirmed')
              }
            }}
          >
            确认继续
          </Button>
          <Button disabled={execBusy} onClick={() => onConfirm('regenerate')}>
            重新生成资产/视觉DNA
          </Button>
        </div>
      )
    }
    if (effectiveInterruptKind === 'confirm_split') {
      return (
        <div style={{ display: 'flex', gap: 8 }}>
          <Button
            variant="primary"
            disabled={execBusy}
            onClick={() => {
              try {
                const text = splitTextareaRef.current?.value ?? splitText
                const parsed = text.trim() ? JSON.parse(text) : null
                onConfirm('confirmed', parsed ? { split_episodes: parsed } : undefined)
              } catch {
                onConfirm('confirmed')
              }
            }}
          >
            确认并写入剧集
          </Button>
          <Button disabled={execBusy} onClick={() => onConfirm('regenerate')}>
            重新分割
          </Button>
        </div>
      )
    }
    if (effectiveInterruptKind === 'confirm_ingest') {
      return (
        <div style={{ display: 'flex', gap: 8 }}>
          <Button variant="primary" disabled={execBusy} onClick={() => onConfirm('confirmed')}>
            确认入库
          </Button>
          <Button disabled={execBusy} onClick={() => onConfirm('rejected')}>
            驳回结束
          </Button>
        </div>
      )
    }
    return null
  }, [assetsText, execBusy, effectiveInterruptKind, onConfirm, splitText])

  const headerStatus = useMemo(() => {
    const locked = episode.script_locked ? '已锁定' : '未锁定'
    const st = episode.exec_status ? String(episode.exec_status) : 'idle'
    return `${locked} / ${st}`
  }, [episode.exec_status, episode.script_locked])

  return (
    <div style={{ marginTop: 12 }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: 10 }}>
        <div style={{ fontSize: 12, fontWeight: 700, opacity: 0.85 }}>执行流程</div>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: 10 }}>
          <div style={{ fontSize: 11, opacity: 0.65 }}>{headerStatus}</div>
          <Button
            variant="primary"
            disabled={execBusy || !!disableExecute || !!episode.script_locked || !String(episode.description || '').trim()}
            onClick={onExecute}
          >
            执行
          </Button>
        </div>
      </div>

      {execRun ? (
        <div style={{ marginTop: 10 }}>
          <RunStatusPanel
            chatRun={execRun}
            uiNowMs={uiNowMs}
            onPausePoll={onPausePoll}
            onResumePoll={onResumePoll}
            onForceRefresh={onForceRefresh}
            chatPollPaused={execPollPaused}
          />
        </div>
      ) : null}

      {effectiveInterruptKind ? (
        <div style={{ marginTop: 10, display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: 10 }}>
          <div style={{ fontSize: 12, opacity: 0.75 }}>等待确认：{effectiveInterruptKind}</div>
          {confirmControls}
        </div>
      ) : null}

      <div style={{ display: 'flex', flexDirection: 'column', gap: 10, marginTop: 10 }}>
        <div>
          <div style={{ fontSize: 12, fontWeight: 700, opacity: 0.85, marginBottom: 6 }}>Step 1：大纲</div>
          <Textarea value={outline} readOnly style={{ height: 160 }} placeholder="等待生成大纲…" />
        </div>

        <div>
          <div style={{ fontSize: 12, fontWeight: 700, opacity: 0.85, marginBottom: 6 }}>Step 2：资产抽离 + 视觉DNA</div>
          {effectiveInterruptKind === 'confirm_assets' ? (
            <Textarea
              ref={assetsTextareaRef}
              defaultValue={assetsText}
              style={{ height: 200 }}
              placeholder="等待生成资产/视觉DNA（JSON）…"
              readOnly={false}
            />
          ) : (
            <Textarea
              value={assetsText}
              style={{ height: 200 }}
              placeholder={
                execRun?.currentActionKey === 'episode_assets_visual_dna'
                  ? 'Step2 生成中…（可在上方“执行步骤”里看进度/错误）'
                  : '等待生成资产/视觉DNA（JSON）…'
              }
              readOnly
            />
          )}
        </div>

        <div>
          <div style={{ fontSize: 12, fontWeight: 700, opacity: 0.85, marginBottom: 6 }}>Step 3：剧集分割 + 各集大纲</div>
          {effectiveInterruptKind === 'confirm_split' ? (
            <Textarea
              ref={splitTextareaRef}
              defaultValue={splitText}
              style={{ height: 200 }}
              placeholder="等待分割结果（JSON）…"
              readOnly={false}
            />
          ) : (
            <Textarea
              value={splitText}
              style={{ height: 200 }}
              placeholder={
                execRun?.currentActionKey === 'episode_split_episodes'
                  ? 'Step3 生成中…（可在上方“执行步骤”里看进度/错误）'
                  : '等待分割结果（JSON）…'
              }
              readOnly
            />
          )}
        </div>

        <div>
          <div style={{ fontSize: 12, fontWeight: 700, opacity: 0.85, marginBottom: 6 }}>Step 4：资产入库预览</div>
          <Textarea
            value={ingestPreview ? JSON.stringify(ingestPreview, null, 2) : ''}
            readOnly
            style={{ height: 120 }}
            placeholder="等待入库预览…"
          />
        </div>
      </div>
    </div>
  )
})
