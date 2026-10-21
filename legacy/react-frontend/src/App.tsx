import { lazy, Suspense } from 'react'
import { Navigate, Route, Routes } from 'react-router-dom'
import { AppLayout } from './components/AppLayout'
import { PlaceholderPage } from './pages/PlaceholderPage'

// 代码分割：使用 React.lazy 进行路由级别的懒加载
const ScriptPage = lazy(() => import('./pages/ScriptPage.refactored').then(m => ({ default: m.ScriptPage })))
const AiSettingsPage = lazy(() => import('./pages/AiSettingsPage').then(m => ({ default: m.AiSettingsPage })))
const EventMatrixPage = lazy(() => import('./pages/EventMatrixPage').then(m => ({ default: m.EventMatrixPage })))
const EventFlowPage = lazy(() => import('./pages/EventFlowPage').then(m => ({ default: m.EventFlowPage })))
const AssetLibraryPage = lazy(() => import('./pages/AssetLibraryPage').then(m => ({ default: m.AssetLibraryPage })))
const PromptsSettingsPage = lazy(() => import('./pages/PromptsSettingsPage').then(m => ({ default: m.PromptsSettingsPage })))
const ContextPage = lazy(() => import('./pages/ContextPage').then(m => ({ default: m.ContextPage })))
const RunInspectorPage = lazy(() => import('./pages/RunInspectorPage').then(m => ({ default: m.RunInspectorPage })))

// 加载中占位组件
function LoadingFallback() {
  return (
    <div style={{ padding: 20, textAlign: 'center', opacity: 0.7 }}>
      加载中...
    </div>
  )
}

function App() {
  return (
    <AppLayout>
      <Suspense fallback={<LoadingFallback />}>
        <Routes>
          <Route path="/" element={<Navigate to="/script" replace />} />
          <Route path="/script" element={<ScriptPage />} />
          <Route path="/events" element={<EventMatrixPage />} />
          <Route path="/events/flow" element={<EventFlowPage />} />
          <Route path="/assets" element={<AssetLibraryPage />} />
          <Route path="/context" element={<ContextPage />} />
          <Route path="/runs" element={<RunInspectorPage />} />
          <Route path="/settings/ai" element={<AiSettingsPage />} />
          <Route path="/settings/prompts" element={<PromptsSettingsPage />} />
          <Route path="*" element={<PlaceholderPage title="404" />} />
        </Routes>
      </Suspense>
    </AppLayout>
  )
}

export default App
