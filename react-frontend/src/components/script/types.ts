/**
 * ScriptPage 相关类型定义
 */

export type Selected =
  | { kind: 'episode'; episodeId: number }
  | { kind: 'scene'; episodeId: number; sceneId: number }
  | { kind: 'shot'; episodeId: number; sceneId: number; shotId: number }
  | { kind: 'none' }

export type ChatRole = 'user' | 'assistant'

export type ChatMsg = {
  id: string
  role: ChatRole
  content: string
  ts: number
  cards?: any[]
  debug?: any
}

export type StepUi = {
  index: number
  action_key: string
  why?: string | null
  status: 'pending' | 'running' | 'done'
  ms?: number
  output_preview?: string
}

export type ChatRunUi = {
  runId: string
  status: 'queued' | 'running' | 'paused' | 'done' | 'error'
  steps: StepUi[]
  error?: string | null
  startedAtMs?: number
  lastAtMs?: number
  currentStepIndex?: number | null
  currentActionKey?: string | null
}

export type DebugLog = {
  id: string
  ts: number
  level: string
  text: string
}

export type BusyState =
  | null
  | 'load'
  | 'save_episode'
  | 'save_scene'
  | 'save_shot'
  | 'create_episode'
  | 'create_scene'
  | 'create_shot'

export type DeletingState = null | 'project' | 'episode'
