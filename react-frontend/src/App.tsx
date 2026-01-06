import { Navigate, Route, Routes } from 'react-router-dom'
import { AppLayout } from './components/AppLayout'
import { ScriptPage } from './pages/ScriptPage'
import { AiSettingsPage } from './pages/AiSettingsPage'
import { PlaceholderPage } from './pages/PlaceholderPage'
import { EventMatrixPage } from './pages/EventMatrixPage'
import { EventFlowPage } from './pages/EventFlowPage'
import { AssetLibraryPage } from './pages/AssetLibraryPage'
import { PromptsSettingsPage } from './pages/PromptsSettingsPage'
import { ContextPage } from './pages/ContextPage'
import { RunInspectorPage } from './pages/RunInspectorPage'

function App() {
  return (
    <AppLayout>
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
    </AppLayout>
  )
}

export default App
