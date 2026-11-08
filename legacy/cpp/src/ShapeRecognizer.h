#pragma once
#include "Common.h"

namespace ShapeRecognizer {
    // 给定笔画点，识别为规则图形；
    // 返回 ShapeKind::Free 表示不识别（保持自由曲线）
    // 若识别成功，输出包围盒（左上 a，右下 b）
    ShapeKind Recognize(const std::vector<PointF2>& pts, PointF2& a, PointF2& b);
}
