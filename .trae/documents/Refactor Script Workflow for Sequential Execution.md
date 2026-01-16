# Refactor Script Workflow for Sequential Execution

I will modify the script workflow to be strictly sequential (Step 0 -> Step 1 -> Step 2), where proceeding to the next step triggers the AI execution automatically.

## 1. UI Updates
- **Step 0 (Script Editor)**:
    - Add a "Structure Breakdown" (结构拆解) button.
    - Clicking this will save the script, navigate to Step 1, and trigger the "Structure Breakdown" AI task.
- **Step 1 (Structure)**:
    - Remove "Generate" and "Show/Hide Code" buttons.
    - Add an "Asset Extraction" (资产抽离) button.
    - Clicking this will navigate to Step 2 and trigger the "Asset Extraction" AI task.
    - **Visual Update**: Render cards using the top-level keys of the JSON output as card titles, and their values as content.
- **Step 2 (Assets)**:
    - Remove "Extract" and "Show/Hide Code" buttons.
    - Display asset cards (Key = Title, Value = Content) based on the AI output.
- **Step 3 (Storyboard)**:
    - Remove this step entirely as requested.
- **Sidebar**:
    - Remove "Step 3".
    - Add a "Delete Episode" button (likely in the episode header or a context menu) to restore that functionality.

## 2. Logic Refactoring (`ScriptPage.refactored.tsx`)
- Implement `goToStep1AndRun`:
    - Saves Step 0 text.
    - Sets selection to Step 1.
    - Triggers `executeEpisode` (which needs to be context-aware or rely on the auto-run logic I previously added, but explicitly triggering is safer for button clicks).
- Implement `goToStep2AndRun`:
    - Sets selection to Step 2.
    - Triggers `executeEpisode`.
- **Delete Episode**:
    - Add a `deleteEpisode` function and pass it to the `ScriptSidebar`.

## 3. Component Interface Updates
- Update props for `Step0`, `Step1`, `Step2` to accept these new navigation/action handlers instead of just generic `onSave` or `onRun`.
- Update `ScriptSidebar` to accept `onDeleteEpisode`.

## 4. Execution Logic
- Ensure `executeEpisode` works correctly for the transition. Since the backend pipeline might be monolithic or state-based, I will ensure the frontend state transitions align with triggering the correct backend flow (or the existing "Run" that does everything is sufficient, and we just view the results stage by stage). *Assumption: The current backend `executeEpisode` runs the full pipeline. I will rely on this and just navigate the view.*

## Plan Steps
1.  **Sidebar**: Add Delete button, remove Step 3.
2.  **Step 0**: Add "Structure Breakdown" button -> `onNext`.
3.  **Step 1**: Clean UI, add "Asset Extraction" button -> `onNext`. Refactor Card display.
4.  **Step 2**: Clean UI. Refactor Card display.
5.  **ScriptPage**: Wire up the `onNext` handlers to perform navigation + execution, and implement `deleteEpisode`.
