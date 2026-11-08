#include "Common.h"
#include "RenderQueue.h"
#include "PdfDoc.h"

#include <thread>
#include <mutex>
#include <condition_variable>
#include <vector>
#include <unordered_set>
#include <algorithm>
#include <roapi.h>

// ====== Internal types ======

struct RenderJob {
    PdfDoc*     pdf;
    RenderTask  task;
};

struct RenderQueue {
    std::vector<std::thread>     workers;
    std::mutex                   mtx;
    std::condition_variable      cv;
    std::vector<RenderJob>       tasks;       // sorted: HIGHEST priority at back (pop_back = O(1))
    std::unordered_set<int>      activeThumbs;
    std::unordered_set<int>      activeFull;
    bool                         shutdown{ false };
    HWND                         hWnd{ nullptr };
    int                          totalWorkers{ 0 };
    int                          uiReserved{ 2 };  // threads reserved for urgent tasks
    int                          busyWorkers{ 0 };  // workers currently executing non-urgent jobs

    bool isAlreadyActive(int pageIdx, bool isThumb) const {
        auto& set = isThumb ? activeThumbs : activeFull;
        return set.count(pageIdx) > 0;
    }
    void markActive(int pageIdx, bool isThumb) {
        auto& set = isThumb ? activeThumbs : activeFull;
        set.insert(pageIdx);
    }
    void unmarkActive(int pageIdx, bool isThumb) {
        auto& set = isThumb ? activeThumbs : activeFull;
        set.erase(pageIdx);
    }

    // Insert sorted by priority: HIGHEST priority (lowest value) goes to BACK
    // This makes pop_back O(1) instead of erase(begin()) O(n)
    void insertSorted(const RenderJob& job) {
        auto it = std::upper_bound(tasks.begin(), tasks.end(), job,
            [](const RenderJob& a, const RenderJob& b) {
                return a.task.priority > b.task.priority;
            });
        tasks.insert(it, job);
    }
};

// ====== Worker thread proc ======

static void WorkerProc(RenderQueue* rq) {
    RoInitialize(RO_INIT_MULTITHREADED);
    SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);

    while (true) {
        RenderJob job;
        {
            std::unique_lock<std::mutex> lock(rq->mtx);

            // Wait for work, but only if we're allowed to take non-urgent work.
            // If all non-reserved slots are busy and there's no urgent work,
            // block this worker so UI stays responsive.
            rq->cv.wait(lock, [&] {
                if (rq->shutdown) return true;
                if (rq->tasks.empty()) return false;

                // Urgent tasks (priority < -500): always wake up
                if (rq->tasks.back().task.priority < -500)
                    return true;

                // Non-urgent: only wake if we have a free slot
                int freeSlots = rq->totalWorkers - rq->busyWorkers - rq->uiReserved;
                return freeSlots > 0;
            });

            if (rq->shutdown && rq->tasks.empty())
                break;

            if (rq->tasks.empty())
                continue;

            // Double-check: skip if no free slot for non-urgent
            bool isUrgent = (rq->tasks.back().task.priority < -500);
            if (!isUrgent) {
                int freeSlots = rq->totalWorkers - rq->busyWorkers - rq->uiReserved;
                if (freeSlots <= 0)
                    continue;
            }

            // Pop highest priority from back (O(1))
            job = std::move(rq->tasks.back());
            rq->tasks.pop_back();
            rq->busyWorkers++;
        }

        // Execute render outside the lock
        std::shared_ptr<Bitmap> resultBmp;
        try {
            Bitmap* raw = job.pdf->RenderPage(job.task.pageIdx, job.task.pixelW, job.task.pixelH);
            if (raw)
                resultBmp.reset(raw);
        } catch (...) {}

        {
            std::lock_guard<std::mutex> lock(rq->mtx);
            rq->unmarkActive(job.task.pageIdx, job.task.isThumbnail);
            rq->busyWorkers--;
        }

        auto* result = new RenderResult{ job.task.pageIdx, resultBmp, job.task.isThumbnail };
        PostMessageW(rq->hWnd, WM_APP_RENDER_DONE, 0, reinterpret_cast<LPARAM>(result));
    }

    RoUninitialize();
}

// ====== Public API ======

RenderQueue* RenderQueue_Create(HWND hWnd) {
    auto* rq = new RenderQueue();
    rq->hWnd = hWnd;

    // Keep PDF rendering conservative. MuPDF access is serialized by PdfDoc,
    // so extra workers mostly contend and steal time from input/painting.
    unsigned int numWorkers = std::thread::hardware_concurrency();
    if (numWorkers < 2) numWorkers = 2;
    if (numWorkers > 2) numWorkers = 2;

    rq->totalWorkers = (int)numWorkers;
    rq->uiReserved = 1;

    rq->workers.reserve(numWorkers);
    for (unsigned int i = 0; i < numWorkers; ++i) {
        rq->workers.emplace_back(WorkerProc, rq);
    }

    return rq;
}

void RenderQueue_Destroy(RenderQueue* rq) {
    if (!rq) return;

    {
        std::lock_guard<std::mutex> lock(rq->mtx);
        rq->shutdown = true;
    }
    rq->cv.notify_all();

    for (auto& w : rq->workers) {
        if (w.joinable())
            w.join();
    }

    delete rq;
}

void RenderQueue_Submit(RenderQueue* rq, PdfDoc* pdf, const RenderTask& task) {
    if (!rq || !pdf) return;

    {
        std::lock_guard<std::mutex> lock(rq->mtx);
        if (rq->shutdown) return;
        if (rq->isAlreadyActive(task.pageIdx, task.isThumbnail)) return;

        RenderJob job{ pdf, task };
        if (task.priority == 0) job.task.priority = task.pageIdx;
        rq->markActive(task.pageIdx, task.isThumbnail);
        rq->insertSorted(job);
    }
    rq->cv.notify_one();
}

void RenderQueue_SubmitBatch(RenderQueue* rq, PdfDoc* pdf,
                             const RenderTask* tasks, int count) {
    if (!rq || !pdf || !tasks || count <= 0) return;

    {
        std::lock_guard<std::mutex> lock(rq->mtx);
        if (rq->shutdown) return;

        for (int i = 0; i < count; ++i) {
            const auto& t = tasks[i];
            if (rq->isAlreadyActive(t.pageIdx, t.isThumbnail)) continue;

            RenderJob job{ pdf, t };
            if (job.task.priority == 0) job.task.priority = t.pageIdx;
            rq->markActive(t.pageIdx, t.isThumbnail);
            rq->insertSorted(job);
        }
    }
    rq->cv.notify_all();
}

void RenderQueue_SubmitUrgent(RenderQueue* rq, PdfDoc* pdf,
                               const RenderTask* tasks, int count) {
    if (!rq || !pdf || !tasks || count <= 0) return;

    {
        std::lock_guard<std::mutex> lock(rq->mtx);
        if (rq->shutdown) return;

        for (int i = 0; i < count; ++i) {
            const auto& t = tasks[i];
            // Cancel any existing lower-priority task for same page
            // (just skip if already active — the new one will be even higher priority)
            if (rq->isAlreadyActive(t.pageIdx, t.isThumbnail)) continue;

            RenderJob job{ pdf, t };
            job.task.priority = -1000 + t.pageIdx;  // very high priority (negative)
            rq->markActive(t.pageIdx, t.isThumbnail);
            rq->insertSorted(job);
        }
    }
    // Wake ALL workers — we want 80% of threads on this
    rq->cv.notify_all();
}

void RenderQueue_CancelAll(RenderQueue* rq) {
    if (!rq) return;

    {
        std::lock_guard<std::mutex> lock(rq->mtx);
        rq->tasks.clear();
        rq->activeThumbs.clear();
        rq->activeFull.clear();
    }
}

void RenderQueue_CancelLowPriority(RenderQueue* rq, int threshold) {
    if (!rq) return;

    {
        std::lock_guard<std::mutex> lock(rq->mtx);

        // Remove tasks with priority > threshold (lower-priority than threshold)
        auto it = std::remove_if(rq->tasks.begin(), rq->tasks.end(),
            [threshold](const RenderJob& job) {
                return job.task.priority > threshold;
            });

        // Also clean up active sets for removed tasks
        for (auto rit = it; rit != rq->tasks.end(); ++rit) {
            rq->unmarkActive(rit->task.pageIdx, rit->task.isThumbnail);
        }
        rq->tasks.erase(it, rq->tasks.end());
    }
}
