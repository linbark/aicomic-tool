export type ID = number

export interface AiSettingsRead {
  has_api_key: boolean
  base_url: string
  model: string
  temperature: number
  max_tokens: number
  timeout_seconds: number
}

export interface AiSettingsUpdate {
  api_key?: string | null
  base_url?: string | null
  model?: string | null
  temperature?: number | null
  max_tokens?: number | null
  timeout_seconds?: number | null
}

export interface AiTestResponse {
  ok: boolean
  detail?: string | null
}

export interface OutlineOptimizeRequest {
  text: string
}

export interface OutlineOptimizeResponse {
  text: string
}

export interface ScriptGenerateRequest {
  text: string
}

export interface ScriptGenerateResponse {
  text: string
}

export interface PromptTemplateRead {
  key: string
  title: string
  category: string
  prompt: string
  is_builtin: boolean
  is_modified: boolean
  variables: string[]
}

export interface PromptTemplateCreate {
  key: string
  title: string
  category: string
  prompt: string
}

export interface PromptTemplateUpsert {
  title: string
  category: string
  prompt: string
}

export interface SplitSceneItem {
  title: string
  description: string
}

export interface SplitShotItem {
  title?: string | null
  action_text: string
}

export interface AssetRead {
  id: ID
  file_path: string
  file_type: string
  meta_data?: Record<string, unknown> | null
  is_favorite?: boolean
  created_at?: string
}

export interface ShotRead {
  id: ID
  sequence_number: number
  title?: string | null
  action_text?: string | null
  dialogue?: string | null
  prompt?: string | null
  status?: string
  selected_asset_id?: ID | null
  video_path?: string | null
  assets?: AssetRead[]
}

export interface SceneRead {
  id: ID
  title?: string | null
  sequence_number?: number | null
  description?: string | null
  action_text?: string | null
  dialogue?: string | null
  prompt?: string | null
  shots?: ShotRead[]
}

export interface EpisodeRead {
  id: ID
  title: string
  order: number
  description?: string | null
  action_text?: string | null
  prompt?: string | null
  scenes?: SceneRead[]
}

export interface ProjectBase {
  id: ID
  name: string
  description?: string | null
}

export interface ProjectCreate {
  name: string
  description?: string | null
}

export interface ProjectUpdate {
  name?: string | null
  description?: string | null
}

export type EventTargetType = 'episode' | 'scene' | 'shot'

export interface EventNodeRead {
  id: ID
  target_type: EventTargetType
  target_id: ID
  description: string
}

export interface EventRead {
  id: ID
  name: string
  color: string
  description?: string | null
  graph_data?: Record<string, unknown> | null
  nodes?: EventNodeRead[]
}

export interface EventCreate {
  name: string
  color?: string
  description?: string | null
}

export interface EventUpdate {
  name?: string | null
  color?: string | null
  description?: string | null
  graph_data?: Record<string, unknown> | null
}
export interface AssetItemRead {
  id: ID
  name: string
  description?: string | null
  base_prompt?: string | null
  category: string
  avatar_asset_id?: ID | null
  assets?: AssetRead[]
}
