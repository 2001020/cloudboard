#include "ShapeRecognizer.h"
#include <numeric>
#include <algorithm>
#include <cmath>
#include <vector>

// ======================================================================
// Ink shape recognition algorithm
// Strategy: Geometric rules first (90%+ coverage, <1ms),
//           lightweight ML fallback when confidence is low.
// ======================================================================

namespace ShapeRecognizer {

// ====== Basic geometry helpers ======

static inline float dist(const PointF2& a, const PointF2& b){
    float dx = a.x - b.x, dy = a.y - b.y;
    return std::sqrt(dx*dx + dy*dy);
}

static float pathLen(const std::vector<PointF2>& pts){
    float s = 0;
    for (size_t i = 1; i < pts.size(); ++i) s += dist(pts[i-1], pts[i]);
    return s;
}

static void bbox(const std::vector<PointF2>& pts, PointF2& mn, PointF2& mx){
    mn.x = mn.y = 1e9f; mx.x = mx.y = -1e9f;
    for (auto& p : pts){
        mn.x = std::min(mn.x, p.x); mn.y = std::min(mn.y, p.y);
        mx.x = std::max(mx.x, p.x); mx.y = std::max(mx.y, p.y);
    }
}

static float triangleArea(float x1, float y1, float x2, float y2, float x3, float y3){
    return std::abs((x2-x1)*(y3-y1) - (x3-x1)*(y2-y1)) * 0.5f;
}

// ====== Step 1: Preprocessing ======

// 1a. Equal-distance resampling (step ~3px)
static std::vector<PointF2> Resample(const std::vector<PointF2>& input, float step = 3.0f){
    if (input.size() < 2) return input;
    std::vector<PointF2> res;
    res.reserve(input.size() * 2);
    res.push_back(input[0]);

    float accum = 0;
    for (size_t i = 1; i < input.size(); i++){
        float dx = input[i].x - input[i-1].x;
        float dy = input[i].y - input[i-1].y;
        float d = std::sqrt(dx*dx + dy*dy);
        if (d < 1e-6f) continue;

        float ux = dx / d, uy = dy / d;
        float remaining = d;
        float start = step - accum;

        if (start > remaining){
            accum += remaining;
            continue;
        }

        float px = input[i-1].x + ux * start;
        float py = input[i-1].y + uy * start;
        res.push_back({px, py});
        remaining -= start;

        while (remaining >= step){
            px += ux * step;
            py += uy * step;
            res.push_back({px, py});
            remaining -= step;
        }
        accum = remaining;
    }
    return res;
}

// 1b. Gaussian smooth (sigma = 2, window radius = 3)
static std::vector<PointF2> GaussianSmooth(const std::vector<PointF2>& pts){
    if (pts.size() < 5) return pts;
    // Gaussian kernel [1,4,7,4,1] / 17  (sigma ~ 1.4)
    static const float k[] = {1.0f/17, 4.0f/17, 7.0f/17, 4.0f/17, 1.0f/17};
    std::vector<PointF2> out(pts.size());
    int n = (int)pts.size();
    for (int i = 0; i < n; i++){
        float sx = 0, sy = 0;
        for (int j = -2; j <= 2; j++){
            int idx = std::max(0, std::min(n-1, i+j));
            sx += pts[idx].x * k[j+2];
            sy += pts[idx].y * k[j+2];
        }
        out[i] = {sx, sy};
    }
    return out;
}

// 1c. Douglas-Peucker simplification
static float perpDist(const PointF2& p, const PointF2& a, const PointF2& b){
    float dx = b.x - a.x, dy = b.y - a.y;
    float len = std::sqrt(dx*dx + dy*dy);
    if (len < 1e-6f) return dist(p, a);
    return std::abs(dy*p.x - dx*p.y + b.x*a.y - b.y*a.x) / len;
}

static void DouglasPeucker(const std::vector<PointF2>& pts, int i, int j,
                           float eps, std::vector<int>& keep){
    float maxD = 0; int idx = -1;
    for (int k = i+1; k < j; ++k){
        float d = perpDist(pts[k], pts[i], pts[j]);
        if (d > maxD){ maxD = d; idx = k; }
    }
    if (maxD > eps && idx >= 0){
        DouglasPeucker(pts, i, idx, eps, keep);
        DouglasPeucker(pts, idx, j, eps, keep);
    } else {
        keep.push_back(j);
    }
}

static std::vector<int> Simplify(const std::vector<PointF2>& pts, float eps){
    std::vector<int> keep;
    keep.push_back(0);
    if (pts.size() > 1)
        DouglasPeucker(pts, 0, (int)pts.size()-1, eps, keep);
    return keep;
}

// ====== Step 2: Feature extraction ======

struct Features {
    float closedness;     // dist(first,last) / diagonal
    float linearity;      // max perpendicular deviation / line length
    float lineR2;         // R-squared of linear fit
    float endDistRatio;   // dist(first,last) / pathLen
    float pathLen;        // total path length
    float diag;           // bounding box diagonal
    int   cornerCount;    // Douglas-Peucker key points
    float areaRatio;      // contour area / bbox area
    float ellipseFitErr;  // mean deviation from fitted ellipse
    float rectFitErr;     // mean distance to nearest bbox edge, normalized
    float aspectRatio;    // width / height of bbox
    float rightAngleScore;// for 4-corner shapes, how close to 90 degrees
    bool  closed;         // closedness < threshold
};

static Features ComputeFeatures(const std::vector<PointF2>& pts,
                                 const std::vector<int>& corners){
    Features f{};
    if (pts.size() < 2) return f;

    PointF2 mn, mx;
    bbox(pts, mn, mx);
    float w = mx.x - mn.x;
    float h = mx.y - mn.y;
    f.diag = std::sqrt(w*w + h*h);
    f.aspectRatio = (h > 1e-3f) ? (w / h) : 1.0f;

    f.pathLen = pathLen(pts);
    float endDist = dist(pts.front(), pts.back());

    // Closedness: first-to-last distance relative to diagonal
    f.closedness = (f.diag > 1e-3f) ? (endDist / f.diag) : 1.0f;
    f.closed = (f.closedness < 0.20f);

    // End distance ratio relative to path length
    f.endDistRatio = (f.pathLen > 1e-3f) ? (endDist / f.pathLen) : 0;

    // Corner count from Douglas-Peucker
    f.cornerCount = (int)corners.size() - 1; // segments = corners for open, = corners for closed+loop

    // Linearity: max deviation from first-last line / line length
    float lineLen = endDist;
    f.linearity = 0;
    if (lineLen > 1e-3f){
        float maxDev = 0;
        for (auto& p : pts){
            float d = perpDist(p, pts.front(), pts.back());
            if (d > maxDev) maxDev = d;
        }
        f.linearity = maxDev / lineLen;
    }

    // Linear fit R-squared
    {
        double mx_ = 0, my_ = 0;
        for (auto& p : pts){ mx_ += p.x; my_ += p.y; }
        mx_ /= pts.size(); my_ /= pts.size();
        double sxx=0, syy=0, sxy=0;
        for (auto& p : pts){
            double dx = p.x - mx_, dy = p.y - my_;
            sxx += dx*dx; syy += dy*dy; sxy += dx*dy;
        }
        double denom = sxx * syy;
        if (denom > 1e-6){
            double r = sxy / std::sqrt(denom);
            f.lineR2 = (float)(r*r);
        } else {
            f.lineR2 = 0;
        }
    }

    // Contour area via shoelace formula
    double area = 0;
    for (size_t i = 0; i < pts.size(); i++){
        size_t j = (i+1) % pts.size();
        area += pts[i].x * pts[j].y - pts[j].x * pts[i].y;
    }
    area = std::abs(area) * 0.5;
    float bboxArea = w * h;
    f.areaRatio = (bboxArea > 1e-3f) ? (float)(area / bboxArea) : 0;

    // Ellipse fit error: normalized deviation from ideal ellipse
    {
        float cx = (mn.x + mx.x) * 0.5f;
        float cy = (mn.y + mx.y) * 0.5f;
        float rx = w * 0.5f, ry = h * 0.5f;
        if (rx < 1) rx = 1; if (ry < 1) ry = 1;

        double devAcc = 0;
        for (auto& p : pts){
            double nx = (p.x - cx) / rx;
            double ny = (p.y - cy) / ry;
            double r = std::sqrt(nx*nx + ny*ny);
            devAcc += std::abs(r - 1.0);
        }
        f.ellipseFitErr = (float)(devAcc / pts.size());
    }

    // Rectangle fit error: mean distance to nearest bbox edge, normalized
    {
        double devAcc = 0;
        for (auto& p : pts){
            double dL = std::abs(p.x - mn.x);
            double dR = std::abs(p.x - mx.x);
            double dT = std::abs(p.y - mn.y);
            double dB = std::abs(p.y - mx.y);
            devAcc += std::min(std::min(dL, dR), std::min(dT, dB));
        }
        f.rectFitErr = (float)(devAcc / pts.size() / std::max(f.diag, 1.0f));
    }

    // Right angle score: for 4-corner shapes, check adjacent edge dot products
    f.rightAngleScore = 0;
    if (f.cornerCount >= 4 && corners.size() >= 5){
        double totalDot = 0;
        int pairs = 0;
        for (size_t i = 0; i + 2 < corners.size(); i++){
            PointF2 a = pts[corners[i]];
            PointF2 b = pts[corners[i+1]];
            PointF2 c = pts[corners[i+2]];
            float v1x = b.x - a.x, v1y = b.y - a.y;
            float v2x = c.x - b.x, v2y = c.y - b.y;
            float len1 = std::sqrt(v1x*v1x + v1y*v1y);
            float len2 = std::sqrt(v2x*v2x + v2y*v2y);
            if (len1 > 1e-3f && len2 > 1e-3f){
                float dot = (v1x*v2x + v1y*v2y) / (len1*len2);
                totalDot += std::abs(dot); // |cos(theta)|, 0 = perfect right angle
                pairs++;
            }
        }
        if (pairs > 0)
            f.rightAngleScore = 1.0f - (float)(totalDot / pairs); // 1.0 = perfect right angles
    }

    return f;
}

// ====== Step 3: Geometric rule-based decision tree ======

// Confidence levels
struct Result {
    ShapeKind kind;
    float confidence; // 0..1
};

static Result RuleDecide(const Features& f, const std::vector<PointF2>& pts,
                          const std::vector<int>& corners){
    // ---- Rule 1: OPEN shapes ----
    if (!f.closed){
        // Line: high linearity (max deviation < 8% of line length) + high R2
        if (f.linearity < 0.08f && f.lineR2 > 0.95f && f.pathLen > 30){
            return { ShapeKind::Line, 0.95f };
        }
        // Loose line: moderate linearity
        if (f.linearity < 0.12f && f.lineR2 > 0.90f && f.endDistRatio > 0.85f){
            return { ShapeKind::Line, 0.80f };
        }
        // Open but not a line
        return { ShapeKind::Free, 0 };
    }

    // ---- Rule 2: CLOSED shapes ----

    // Very small: ignore
    if (f.diag < 12) return { ShapeKind::Free, 0 };

    // Circle/Ellipse: low ellipse fit error
    if (f.ellipseFitErr < 0.12f){
        // Distinguish circle vs ellipse by aspect ratio
        float ar = std::max(f.aspectRatio, 1.0f / std::max(f.aspectRatio, 0.01f));
        if (ar < 1.3f)
            return { ShapeKind::Ellipse, 0.95f }; // nearly circular ellipse
        else
            return { ShapeKind::Ellipse, 0.90f };
    }

    // Line (nearly closed but actually a back-and-forth stroke)
    if (f.linearity < 0.06f && f.lineR2 > 0.97f && f.pathLen > 30){
        return { ShapeKind::Line, 0.85f };
    }

    // Triangle: 3 corners (after simplification) + reasonable area
    if (f.cornerCount == 3){
        return { ShapeKind::Triangle, 0.88f };
    }

    // Rectangle: 4 corners + good right angle score + low rect fit error
    if (f.cornerCount == 4){
        if (f.rightAngleScore > 0.65f && f.rectFitErr < 0.06f){
            return { ShapeKind::Rect, 0.92f };
        }
        // Could be a diamond/rhombus — still treat as rectangle
        if (f.rectFitErr < 0.08f){
            return { ShapeKind::Rect, 0.80f };
        }
    }

    // 5+ corners but still might be an ellipse (noisy circle)
    if (f.cornerCount >= 5 && f.ellipseFitErr < 0.18f && f.areaRatio > 0.65f){
        return { ShapeKind::Ellipse, 0.78f };
    }

    // 4 corners but not right-angled — could be parallelogram/trapezoid, use Rect
    if (f.cornerCount == 4 && f.areaRatio > 0.5f){
        return { ShapeKind::Rect, 0.70f };
    }

    // Triangle with 4 corners (one extra from noise)
    if (f.cornerCount == 4 && corners.size() >= 5){
        // Check if one corner is very close to the line between its neighbors
        for (size_t i = 0; i < corners.size()-1; i++){
            int ci = corners[i];
            int cj = corners[(i+1) % (corners.size()-1)];
            int ck = corners[(i+2) % (corners.size()-1)];
            if (ck == corners.back() && (i+2) >= corners.size()-1) break;
            if (ck >= (int)pts.size() || cj >= (int)pts.size() || ci >= (int)pts.size()) continue;
            float d = perpDist(pts[cj], pts[ci], pts[ck]);
            if (d < f.diag * 0.05f){
                // This corner is noise — it's actually a triangle
                return { ShapeKind::Triangle, 0.75f };
            }
        }
    }

    return { ShapeKind::Free, 0 };
}

// ====== Step 4: Lightweight ML fallback (mock) ======
// Feature vector: {closedness, cornerCount/10, areaRatio, ellipseFitErr,
//                  rectFitErr, aspectRatio, linearity, rightAngleScore}
// A trained linear SVM or small random forest would go here.
// For now, use a simple weighted scoring approach.

static Result MLFallback(const Features& f){
    // Score each shape class based on feature weights
    // These weights simulate a trained classifier

    struct Score { ShapeKind kind; float s; };
    Score scores[4] = {
        { ShapeKind::Line,      0 },
        { ShapeKind::Ellipse,   0 },
        { ShapeKind::Triangle,  0 },
        { ShapeKind::Rect,      0 },
    };

    // Line scoring
    if (!f.closed){
        scores[0].s += (1.0f - f.linearity) * 3.0f;
        scores[0].s += f.lineR2 * 2.0f;
        scores[0].s += f.endDistRatio * 1.5f;
    }

    // Ellipse scoring
    if (f.closed){
        scores[1].s += (1.0f - f.ellipseFitErr) * 4.0f;
        scores[1].s += f.areaRatio * 1.5f;
        if (f.cornerCount >= 5) scores[1].s += 1.0f;
    }

    // Triangle scoring
    if (f.closed){
        if (f.cornerCount == 3) scores[2].s += 4.0f;
        if (f.cornerCount == 4) scores[2].s += 1.0f; // might be noisy
        scores[2].s += f.areaRatio * 0.5f;
    }

    // Rectangle scoring
    if (f.closed){
        if (f.cornerCount == 4) scores[3].s += 3.0f;
        scores[3].s += f.rightAngleScore * 2.0f;
        scores[3].s += (1.0f - f.rectFitErr) * 2.0f;
    }

    // Find best
    Score* best = &scores[0];
    for (int i = 1; i < 4; i++){
        if (scores[i].s > best->s) best = &scores[i];
    }

    // Convert score to confidence (sigmoid-like mapping)
    float conf = std::min(1.0f, best->s / 8.0f);
    return { best->kind, conf };
}

// ====== Main entry point ======

ShapeKind Recognize(const std::vector<PointF2>& rawPts, PointF2& a, PointF2& b){
    if (rawPts.size() < 6) return ShapeKind::Free;

    // Step 1: Preprocessing
    std::vector<PointF2> pts = Resample(rawPts, 3.0f);
    pts = GaussianSmooth(pts);

    PointF2 mn, mx;
    bbox(pts, mn, mx);
    float w = mx.x - mn.x, h = mx.y - mn.y;
    float diag = std::sqrt(w*w + h*h);
    if (diag < 12) return ShapeKind::Free; // too small / jitter

    // Douglas-Peucker with epsilon = 3.5% of diagonal
    std::vector<int> corners = Simplify(pts, diag * 0.035f);

    // Step 2: Feature extraction
    Features f = ComputeFeatures(pts, corners);

    // Step 3: Rule-based decision (fast path)
    Result rule = RuleDecide(f, pts, corners);
    if (rule.confidence >= 0.80f){
        a = mn; b = mx;
        if (rule.kind == ShapeKind::Line){
            a = rawPts.front(); b = rawPts.back();
        }
        return rule.kind;
    }

    // Step 4: ML fallback (only when rules are uncertain)
    Result ml = MLFallback(f);
    if (ml.confidence >= 0.50f){
        a = mn; b = mx;
        if (ml.kind == ShapeKind::Line){
            a = rawPts.front(); b = rawPts.back();
        }
        return ml.kind;
    }

    // No confident match — keep free stroke
    return ShapeKind::Free;
}

} // namespace ShapeRecognizer
