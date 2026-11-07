#pragma once
#include "Common.h"

// Opaque render queue (thread pool for parallel PDF page rendering).
// Workers render pages via PdfDoc::RenderPage and post results back
// to the UI thread via PostMessage(WM_APP_RENDER_DONE).
//
// Thread pool design:
//   - Total workers = hardware_concurrency (min 2, max 16)
//   - 2 threads reserved for UI-side tasks (rendering on demand for
//     immediate display, e.g. when user jumps to an unrendered page)
//   - Remaining threads handle background rendering in priority order:
//     earlier pages first, later pages get fewer resources
//   - Priority: lower pageIdx = higher priority (front of queue)

class PdfDoc;  // forward declaration

struct RenderQueue;

struct RenderTask {
    int  pageIdx;
    int  pixelW;
    int  pixelH;
    bool isThumbnail;
    int  priority;  // lower = higher priority; defaults to pageIdx
};

// Create a thread pool with hardware_concurrency() workers.
// hWnd: window to receive WM_APP_RENDER_DONE messages.
RenderQueue* RenderQueue_Create(HWND hWnd);

// Shut down thread pool, join all workers. Call before deleting PdfDoc.
void RenderQueue_Destroy(RenderQueue* rq);

// Submit a single render task.
void RenderQueue_Submit(RenderQueue* rq, PdfDoc* pdf, const RenderTask& task);

// Submit multiple tasks at once (more efficient).
void RenderQueue_SubmitBatch(RenderQueue* rq, PdfDoc* pdf,
                             const RenderTask* tasks, int count);

// Submit urgent high-priority tasks (e.g. current page + neighbors when
// user jumps to an unrendered page). These go to front of queue.
void RenderQueue_SubmitUrgent(RenderQueue* rq, PdfDoc* pdf,
                               const RenderTask* tasks, int count);

// Cancel all pending tasks. In-flight tasks will still complete and post results.
void RenderQueue_CancelAll(RenderQueue* rq);

// Cancel only tasks with priority > threshold (keep urgent/near tasks).
void RenderQueue_CancelLowPriority(RenderQueue* rq, int threshold);
