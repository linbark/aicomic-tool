import { memo, useEffect, useMemo, useState } from 'react'
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
  busy: boolean
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
  busy,
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

  const [assetsJson, setAssetsJson] = useState(() =>
    assetsVisualDna ? JSON.stringify(assetsVisualDna, null, 2) : ''
  )
  const [splitJson, setSplitJson] = useState(() => (splitEpisodes ? JSON.stringify(splitEpisodes, null, 2) : ''))

  useEffect(() => {
    setAssetsJson(assetsVisualDna ? JSON.stringify(assetsVisualDna, null, 2) : '')
  }, [assetsVisualDna])

  useEffect(() => {
    const raw = String(rawAssetsVisualDnaText || '').trim()
    if (assetsVisualDna) return
    if (!raw) return
    setAssetsJson((prev) => (String(prev || '').trim() ? prev : raw))
  }, [assetsVisualDna, rawAssetsVisualDnaText])

  useEffect(() => {
    setSplitJson(splitEpisodes ? JSON.stringify(splitEpisodes, null, 2) : '')
  }, [splitEpisodes])

  useEffect(() => {
    const raw = String(rawSplitEpisodesText || '').trim()
    if (splitEpisodes) return
    if (!raw) return
    setSplitJson((prev) => (String(prev || '').trim() ? prev : raw))
  }, [rawSplitEpisodesText, splitEpisodes])

  const confirmControls = useMemo(() => {
    if (!effectiveInterruptKind) return null
    if (effectiveInterruptKind === 'confirm_outline') {
      return (
        <div style={{ display: 'flex', gap: 8 }}>
          <Button variant="primary" disabled={busy} onClick={() => onConfirm('confirmed')}>
            确认继续
          </Button>
          <Button disabled={busy} onClick={() => onConfirm('regenerate')}>
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
            disabled={busy}
            onClick={() => {
              try {
                const parsed = assetsJson.trim() ? JSON.parse(assetsJson) : null
                onConfirm('confirmed', parsed ? { assets_visual_dna: parsed } : undefined)
              } catch {
                onConfirm('confirmed')
              }
            }}
          >
            确认继续
          </Button>
          <Button disabled={busy} onClick={() => onConfirm('regenerate')}>
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
            disabled={busy}
            onClick={() => {
              try {
                const parsed = splitJson.trim() ? JSON.parse(splitJson) : null
                onConfirm('confirmed', parsed ? { split_episodes: parsed } : undefined)
              } catch {
                onConfirm('confirmed')
              }
            }}
          >
            确认并写入剧集
          </Button>
          <Button disabled={busy} onClick={() => onConfirm('regenerate')}>
            重新分割
          </Button>
        </div>
      )
    }
    if (effectiveInterruptKind === 'confirm_ingest') {
      return (
        <div style={{ display: 'flex', gap: 8 }}>
          <Button variant="primary" disabled={busy} onClick={() => onConfirm('confirmed')}>
            确认入库
          </Button>
          <Button disabled={busy} onClick={() => onConfirm('rejected')}>
            驳回结束
          </Button>
        </div>
      )
    }
    return null
  }, [assetsJson, busy, effectiveInterruptKind, onConfirm, splitJson])

  const headerStatus = useMemo(() => {
    const locked = episode.script_locked ? '已锁定' : '未锁定'
    const st = episode.exec_status ? String(episode.exec_status) : 'idle'
    return `${locked} / ${st}`
  }, [episode.exec_status, episode.script_locked])

  return (
    <div style={{ marginTop: 12 }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: 10 }}>
        <div style={{ fontSize: 12, fontWeight: 700, opacity: 0.85 }}>执行流程</div>
        <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
          <div style={{ fontSize: 11, opacity: 0.65 }}>{headerStatus}</div>
          <Button variant="primary" disabled={busy || !!episode.script_locked || !String(episode.description || '').trim()} onClick={onExecute}>
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
          <Textarea
            value={
              effectiveInterruptKind === 'confirm_assets'
                ? assetsJson
                : assetsVisualDna
                  ? JSON.stringify(assetsVisualDna, null, 2)
                  : assetsJson
            }
            onChange={(e) => setAssetsJson(e.target.value)}
            style={{ height: 200 }}
            placeholder={
              execRun?.currentActionKey === 'episode_assets_visual_dna'
                ? 'Step2 生成中…（可在上方“执行步骤”里看进度/错误）'
                : '等待生成资产/视觉DNA（JSON）…'
            }
            readOnly={effectiveInterruptKind !== 'confirm_assets'}
          />
        </div>

        <div>
          <div style={{ fontSize: 12, fontWeight: 700, opacity: 0.85, marginBottom: 6 }}>Step 3：剧集分割 + 各集大纲</div>
          <Textarea
            value={
              effectiveInterruptKind === 'confirm_split'
                ? splitJson
                : splitEpisodes
                  ? JSON.stringify(splitEpisodes, null, 2)
                  : splitJson
            }
            onChange={(e) => setSplitJson(e.target.value)}
            style={{ height: 200 }}
            placeholder={
              execRun?.currentActionKey === 'episode_split_episodes'
                ? 'Step3 生成中…（可在上方“执行步骤”里看进度/错误）'
                : '等待分割结果（JSON）…'
            }
            readOnly={effectiveInterruptKind !== 'confirm_split'}
          />
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
