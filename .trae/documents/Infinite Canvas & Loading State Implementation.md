# Infinite Canvas & Loading State Implementation Plan

I will implement the "Infinite Canvas" functionality using `react-zoom-pan-pinch` and add a "Generating" loading overlay state.

## 1. Dependency Installation
- Install `react-zoom-pan-pinch`: A lightweight and robust library for pan and zoom interactions in React.

## 2. Component Updates

### A. `SharedStepLayout.tsx`
- **Integrate `TransformWrapper` & `TransformComponent`**:
  - Wrap the `children` (the canvas content) with these components.
  - Configure options: `initialScale={1}`, `minScale={0.5}`, `maxScale={4}`, `centerOnInit={true}`, `limitToBounds={false}` (true "infinite" feel).
  - Add "Zoom In/Out/Reset" controls to the Floating Toolbar (optional but recommended for UX).
- **Add Loading Overlay**:
  - Add a `busy` prop to `SharedStepLayout`.
  - When `busy` is true:
    - Render a semi-transparent overlay over the canvas area.
    - Show a spinner/loading animation and text "正在生成中..." (Generating...).
    - Disable interactions (pointer-events: none) on the canvas content.

### B. `Step1_Structure.tsx` & `Step2_Assets.tsx`
- Pass the `busy` prop down to `SharedStepLayout`.
- No major changes needed to the content rendering itself, as `react-zoom-pan-pinch` handles the container transformation.

## 3. Implementation Steps
1.  **Install**: `npm install react-zoom-pan-pinch` in `react-frontend`.
2.  **Modify Layout**: Update `SharedStepLayout.tsx` to include the library and the loading state logic.
3.  **Update Steps**: Ensure `Step1` and `Step2` pass the `busy` flag correctly.
4.  **Verify**: Check that the canvas can be panned/zoomed and that the loading state blocks interaction visually and functionally.
