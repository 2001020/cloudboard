#pragma once
#include "Common.h"
#include <gl/GL.h>

// Missing OpenGL 1.2+ constants (Microsoft's gl.h only covers 1.1)
#ifndef GL_BGRA
#define GL_BGRA                          0x80E1
#endif
#ifndef GL_CLAMP_TO_EDGE
#define GL_CLAMP_TO_EDGE                 0x812F
#endif
#ifndef GL_FRAMEBUFFER
#define GL_FRAMEBUFFER                   0x8D40
#endif
#ifndef GL_COLOR_ATTACHMENT0
#define GL_COLOR_ATTACHMENT0             0x8CE0
#endif
#ifndef GL_FRAMEBUFFER_COMPLETE
#define GL_FRAMEBUFFER_COMPLETE          0x8CD5
#endif
#ifndef GL_PIXEL_PACK_BUFFER
#define GL_PIXEL_PACK_BUFFER             0x88EB
#endif
#ifndef GL_STREAM_READ
#define GL_STREAM_READ                   0x88E1
#endif
#ifndef GL_READ_ONLY
#define GL_READ_ONLY                     0x88B8
#endif
#ifndef GL_RGBA8
#define GL_RGBA8                         0x8058
#endif

// Missing type (glext.h)
#ifndef GLsizeiptr
#include <cstddef>
typedef ptrdiff_t GLsizeiptr;
#endif

// OpenGL offscreen renderer for board content.
// Renders to an FBO, then reads back pixels for GDI+ compositing.

struct GLRenderContext {
    HWND   hWnd = nullptr;    // hidden window for GL context
    HDC    hDC = nullptr;     // DC for hidden window
    HGLRC  hRC = nullptr;     // GL rendering context
    GLuint fbo = 0;           // framebuffer object
    GLuint fboTex = 0;        // color attachment texture
    int    fboW = 0, fboH = 0;
    std::vector<BYTE> readBuf; // pixel buffer for synchronous glReadPixels
};

extern GLRenderContext glc;

// Lifecycle
bool GLInit();
void GLShutdown();
void GLResize(int w, int h);

// Frame — call GLBeginFrame / GLEndFrame around board rendering.
// After GLEndFrame, glc.readBuf contains BGRA pixel data.
void GLBeginFrame(int w, int h);
void GLEndFrame();
bool GLFrameValid();  // true if FBO is ready

// Transforms (match GDI+ Matrix: Scale(z) * Translate(tx, ty))
void GLPushTransform(float scaleX, float scaleY, float translateX, float translateY);
void GLPopTransform();

// Clip (axis-aligned scissor)
void GLPushClip(int x, int y, int w, int h);
void GLPopClip();

// Primitives — all coordinates in current transform space
void GLFillRect(float x, float y, float w, float h, Gdiplus::Color c);
void GLDrawRect(float x, float y, float w, float h, Gdiplus::Color c, float sw = 1.f);
void GLFillEllipse(float cx, float cy, float rx, float ry, Gdiplus::Color c);
void GLDrawEllipse(float cx, float cy, float rx, float ry, Gdiplus::Color c, float sw = 1.f);
void GLDrawLine(float x1, float y1, float x2, float y2, Gdiplus::Color c, float sw = 1.f);

// Thick polyline stroke (antialiased, round-ish caps via point sprites)
void GLDrawStroke(const std::vector<PointF2>& pts, Gdiplus::Color c, float width, bool dashed = false);

// Shape strokes
void GLDrawShapeRect(float x, float y, float w, float h, Gdiplus::Color c, float sw, bool dashed);
void GLDrawShapeEllipse(float x, float y, float w, float h, Gdiplus::Color c, float sw, bool dashed);
void GLDrawShapeTriangle(float x1, float y1, float x2, float y2, Gdiplus::Color c, float sw, bool dashed);
void GLDrawShapeLine(float x1, float y1, float x2, float y2, Gdiplus::Color c, float sw);
void GLDrawShapeArrow(float x1, float y1, float x2, float y2, Gdiplus::Color c, float sw);

// Texture — data must be BGRA byte order (matching GDI+ 32bppRGB/ARGB)
GLuint GLCreateTexture(int w, int h, const void* data, bool linear = true);
void GLUpdateTexture(GLuint tex, int w, int h, const void* data);
void GLDrawTexture(GLuint tex, float x, float y, float w, float h, bool bilinear = true);
void GLDeleteTexture(GLuint tex);

// Rectangle with dash pattern
void GLDrawDashedRect(float x, float y, float w, float h, Gdiplus::Color c, float sw);

// Selection rubber band (filled rect + border)
void GLDrawRubberBand(float x, float y, float w, float h,
    Gdiplus::Color fill, Gdiplus::Color border, float sw);
