# Refactor Frontend Interface: IDE-like Workflow

I will refactor the `ScriptPage` to implement the requested IDE-like layout with a Sidebar (Project -> Episode -> Steps) and a Multi-view Workspace.

## 1. Type Definitions
- Update `src/components/script/types.ts` to define `ScriptStep` (0-3) and update `Selected` state to track `episodeId` and `step`.

## 2. Component Structure
I will create a new directory `src/components/script/steps/` and implement the following components:

### A. Sidebar
- **`ScriptSidebar.tsx`**: Replaces `EpisodeList`. Renders the tree structure:
  - Project (Root)
    - Episode (Collapsible)
      - Step 0: 原始剧本
      - Step 1: 结构化拆解
      - Step 2: 资产提取
      - Step 3: 分镜细化

### B. Step Components
- **`Step0_Script.tsx`**: Wraps the existing script editing logic (Textarea).
- **`Step1_Structure.tsx`**: Implements "Structure & Flow" view.
  - Top: Floating Code Window (Textarea/CodeMirror) for `rawSplitEpisodesText`.
  - Main: Infinite Canvas (simulated with scrollable area) showing structure cards.
- **`Step2_Assets.tsx`**: Implements "Asset Extraction" view.
  - Top: Floating Code Window for `rawAssetsVisualDnaText`.
  - Main: Asset Grid/Gallery.
- **`Step3_Storyboard.tsx`**: Implements "Storyboard Canvas".
  - Top: Floating Code Window for Scene/Shot JSON data.
  - Main: Flow Canvas showing Scene/Shot cards.
  - Functionality: Replaces `SceneEditor` and `ShotEditor` with a card-based interface.

### C. Shared Layout
- **`SharedStepLayout.tsx`**: A wrapper component for Steps 1-3 providing the "Floating Code Window" and "Floating Toolbar" layout.

## 3. Refactoring `ScriptPage`
- Modify `src/pages/ScriptPage.refactored.tsx`:
  - Adopt the new 2-column layout (Sidebar + Workspace).
  - Replace `EpisodeList` with `ScriptSidebar`.
  - Replace the current detailed editor logic with a switch-case rendering the selected `Step` component.
  - Pass necessary data (`episodes`, `execRun`, `rawTexts`, handlers) down to the Step components.

## 4. Implementation Steps
1.  **Update Types**: Modify `types.ts`.
2.  **Create Sidebar**: Build `ScriptSidebar.tsx`.
3.  **Create Step Components**: Build `Step0` to `Step3` with basic layouts and data binding.
4.  **Integrate**: Update `ScriptPage.refactored.tsx` to assemble the new UI.
5.  **Verify**: Ensure all steps render correctly and data persistence (Save/Run) still works.
