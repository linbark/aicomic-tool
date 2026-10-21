import type { ReactNode } from 'react'
import { useRef, useState } from 'react'
import { TransformWrapper, TransformComponent, useControls, useTransformComponent } from 'react-zoom-pan-pinch'
import { Button } from '../../ui/Button'

type SharedStepLayoutProps = {
  children: ReactNode // The main canvas content
  codeWindow?: ReactNode // The floating code window content
  toolbar?: ReactNode // The floating toolbar buttons
  busy?: boolean // If true, show loading overlay
  enableInfiniteCanvas?: boolean // If true, wrap content in infinite canvas
}

type DragOffset = { x: number; y: number }

// Internal component to access zoom controls context
const ZoomControls = () => {
  const { zoomIn, zoomOut, resetTransform } = useControls();
  return (
    <div style={{ display: 'flex', gap: 4, background: 'rgba(0,0,0,0.3)', padding: 4, borderRadius: 6 }}>
      <Button onClick={() => zoomIn()} style={{ padding: '4px 8px', fontSize: 12 }}>+</Button>
      <Button onClick={() => zoomOut()} style={{ padding: '4px 8px', fontSize: 12 }}>-</Button>
      <Button onClick={() => resetTransform()} style={{ padding: '4px 8px', fontSize: 12 }}>R</Button>
    </div>
  );
};

const DraggablePanel = ({
  children,
  panelStyle,
  disabled,
  scale = 1,
}: {
  children: ReactNode
  panelStyle: React.CSSProperties
  disabled?: boolean
  scale?: number
}) => {
  const [offset, setOffset] = useState<DragOffset>({ x: 0, y: 0 })
  const draggingRef = useRef<{
    pointerId: number
    startClientX: number
    startClientY: number
    startOffsetX: number
    startOffsetY: number
  } | null>(null)

  return (
    <div
      className="aicomic-draggable-panel"
      style={{
        ...panelStyle,
        transform: `translate3d(${offset.x}px, ${offset.y}px, 0)`,
        willChange: 'transform',
      }}
      onPointerDown={(e) => {
        if (disabled) return
        const target = e.target as HTMLElement | null
        const handle = target?.closest?.('[data-aicomic-drag-handle="true"]') as HTMLElement | null
        if (!handle) return

        e.preventDefault()
        e.stopPropagation()
        e.currentTarget.setPointerCapture(e.pointerId)
        draggingRef.current = {
          pointerId: e.pointerId,
          startClientX: e.clientX,
          startClientY: e.clientY,
          startOffsetX: offset.x,
          startOffsetY: offset.y,
        }
      }}
      onPointerMove={(e) => {
        const dragging = draggingRef.current
        if (!dragging || dragging.pointerId !== e.pointerId) return
        e.preventDefault()
        e.stopPropagation()
        const s = Number.isFinite(scale) && scale > 0 ? scale : 1
        const dx = (e.clientX - dragging.startClientX) / s
        const dy = (e.clientY - dragging.startClientY) / s
        setOffset({ x: dragging.startOffsetX + dx, y: dragging.startOffsetY + dy })
      }}
      onPointerUp={(e) => {
        const dragging = draggingRef.current
        if (!dragging || dragging.pointerId !== e.pointerId) return
        e.preventDefault()
        e.stopPropagation()
        try {
          e.currentTarget.releasePointerCapture(e.pointerId)
        } catch (err) {
          void err
        }
        draggingRef.current = null
      }}
      onPointerCancel={(e) => {
        const dragging = draggingRef.current
        if (!dragging || dragging.pointerId !== e.pointerId) return
        e.preventDefault()
        e.stopPropagation()
        try {
          e.currentTarget.releasePointerCapture(e.pointerId)
        } catch (err) {
          void err
        }
        draggingRef.current = null
      }}
    >
      {children}
    </div>
  )
}

export const CanvasDraggablePanel = ({
  children,
  panelStyle,
  disabled,
}: {
  children: ReactNode
  panelStyle: React.CSSProperties
  disabled?: boolean
}) => {
  const scale = useTransformComponent((s) => s.state.scale)
  return (
    <DraggablePanel panelStyle={panelStyle} disabled={disabled} scale={scale}>
      {children}
    </DraggablePanel>
  )
}

export function SharedStepLayout({ 
  children, 
  codeWindow, 
  toolbar, 
  busy,
  enableInfiniteCanvas 
}: SharedStepLayoutProps) {
  return (
    <div style={styles.container}>
      {/* Main Canvas Area */}
      <div style={styles.canvas}>
        {enableInfiniteCanvas ? (
          <TransformWrapper
            initialScale={1}
            minScale={0.1}
            maxScale={4}
            centerOnInit={true}
            limitToBounds={false}
            disabled={false} 
            panning={{
              disabled: busy,
              allowLeftClickPan: true,
              excluded: ['aicomic-draggable-panel', 'textarea', 'input', 'button', 'select'],
            }} 
            wheel={{ step: 0.1, disabled: busy, excluded: ['aicomic-draggable-panel', 'textarea', 'input', 'button', 'select'] }}
          >
            {() => (
              <>
                <TransformComponent
                  wrapperStyle={{
                    width: '100%',
                    height: '100%',
                  }}
                  contentStyle={{
                    minWidth: '100%',
                    minHeight: '100%',
                    // Grid Background Pattern
                    backgroundImage: 'radial-gradient(rgba(255, 255, 255, 0.1) 1px, transparent 1px)',
                    backgroundSize: '20px 20px',
                    backgroundColor: '#1e1e1e', // Dark Gray Background
                  }}
                >
                  <div style={styles.infiniteContent}>
                    {/* Code Window is now part of the canvas content to move with it */}
                    {codeWindow && (
                        <CanvasDraggablePanel panelStyle={styles.canvasCodeWindow} disabled={busy}>
                          {codeWindow}
                        </CanvasDraggablePanel>
                    )}
                    {children}
                  </div>
                </TransformComponent>
                
                {/* Floating Toolbar (Fixed Position on Screen) */}
                <div style={styles.toolbar}>
                  <ZoomControls />
                  {toolbar}
                </div>
              </>
            )}
          </TransformWrapper>
        ) : (
          <>
             {children}
             {/* Floating Toolbar for non-canvas steps */}
             {toolbar && (
                <div style={styles.toolbar}>
                  {toolbar}
                </div>
              )}
             {codeWindow && (
                <DraggablePanel panelStyle={styles.codeWindow} disabled={busy}>
                  {codeWindow}
                </DraggablePanel>
             )}
          </>
        )}
      </div>

      {/* Busy Overlay */}
      {busy && (
        <div style={styles.overlay}>
          <div style={styles.spinnerContainer}>
            <div className="aicomic-spinner" style={styles.spinner}></div>
            <div style={styles.spinnerText}>正在生成中...</div>
          </div>
        </div>
      )}
    </div>
  )
}

const styles: Record<string, React.CSSProperties> = {
  container: {
    position: 'relative',
    width: '100%',
    height: '100%',
    overflow: 'hidden',
    background: '#111', 
    borderRadius: 12,
    border: '1px solid rgba(255,255,255,0.08)',
  },
  canvas: {
    width: '100%',
    height: '100%',
    overflow: 'hidden', 
  },
  infiniteContent: {
      padding: '2000px', // Huge padding for infinite feel
      display: 'flex',
      alignItems: 'flex-start',
      justifyContent: 'center',
      position: 'relative',
  },
  overlay: {
    position: 'absolute',
    top: 0,
    left: 0,
    right: 0,
    bottom: 0,
    background: 'rgba(0,0,0,0.6)',
    backdropFilter: 'blur(4px)',
    zIndex: 50, 
    display: 'flex',
    justifyContent: 'center',
    alignItems: 'center',
    pointerEvents: 'all', 
    cursor: 'wait',
  },
  spinnerContainer: {
    display: 'flex',
    flexDirection: 'column',
    alignItems: 'center',
    gap: 16,
  },
  spinner: {
    width: 40,
    height: 40,
    border: '4px solid rgba(255,255,255,0.1)',
    borderTop: '4px solid #6366f1',
    borderRadius: '50%',
    animation: 'spin 1s linear infinite',
  },
  spinnerText: {
    color: '#fff',
    fontSize: 16,
    fontWeight: 600,
    textShadow: '0 2px 4px rgba(0,0,0,0.5)',
  },
  // Style for code window when it's fixed (non-infinite canvas)
  codeWindow: {
    position: 'absolute',
    top: 20,
    left: 20,
    width: '45vw', 
    maxWidth: 800,
    minWidth: 400,
    height: '60vh', 
    maxHeight: '80%',
    background: 'rgba(30, 30, 30, 0.98)',
    border: '1px solid rgba(255,255,255,0.15)',
    borderRadius: 8,
    boxShadow: '0 8px 30px rgba(0,0,0,0.6)',
    display: 'flex',
    flexDirection: 'column',
    zIndex: 10,
    backdropFilter: 'blur(10px)',
  },
  // Style for code window when it's inside the canvas
  canvasCodeWindow: {
    position: 'absolute',
    top: 120,
    left: 120,
    width: 500,
    height: 600,
    background: 'rgba(30, 30, 30, 0.98)',
    border: '1px solid rgba(255,255,255,0.15)',
    borderRadius: 8,
    boxShadow: '0 8px 30px rgba(0,0,0,0.6)',
    display: 'flex',
    flexDirection: 'column',
    flexShrink: 0, // Don't shrink
    zIndex: 5,
  },
  toolbar: {
    position: 'absolute',
    top: 20,
    right: 20,
    display: 'flex',
    gap: 8,
    zIndex: 10,
    background: 'rgba(30, 30, 30, 0.8)',
    padding: '6px 10px',
    borderRadius: 8,
    border: '1px solid rgba(255,255,255,0.1)',
    backdropFilter: 'blur(4px)',
  }
}

// Add global style for spinner animation if not present
if (typeof document !== 'undefined') {
  const styleSheet = document.createElement("style");
  styleSheet.innerText = `
    @keyframes spin {
      0% { transform: rotate(0deg); }
      100% { transform: rotate(360deg); }
    }
  `;
  document.head.appendChild(styleSheet);
}
