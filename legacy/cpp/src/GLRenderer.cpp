#include "GLRenderer.h"
#include <cstring>
#include <cmath>

GLRenderContext glc;

// ====== FBO extension function pointers ======
typedef void (APIENTRY* PFN_glGenFramebuffers)(GLsizei, GLuint*);
typedef void (APIENTRY* PFN_glBindFramebuffer)(GLenum, GLuint);
typedef void (APIENTRY* PFN_glFramebufferTexture2D)(GLenum, GLenum, GLenum, GLuint, GLint);
typedef GLenum (APIENTRY* PFN_glCheckFramebufferStatus)(GLenum);
typedef void (APIENTRY* PFN_glDeleteFramebuffers)(GLsizei, const GLuint*);

static PFN_glGenFramebuffers        pglGenFramebuffers = nullptr;
static PFN_glBindFramebuffer        pglBindFramebuffer = nullptr;
static PFN_glFramebufferTexture2D   pglFramebufferTexture2D = nullptr;
static PFN_glCheckFramebufferStatus pglCheckFramebufferStatus = nullptr;
static PFN_glDeleteFramebuffers     pglDeleteFramebuffers = nullptr;

static bool LoadGLExtensions(){
    #define LOAD(fn) p##fn = (PFN_##fn)wglGetProcAddress("gl" #fn); if (!p##fn) return false
    LOAD(glGenFramebuffers);
    LOAD(glBindFramebuffer);
    LOAD(glFramebufferTexture2D);
    LOAD(glCheckFramebufferStatus);
    LOAD(glDeleteFramebuffers);
    #undef LOAD
    return true;
}

// ====== Lifecycle ======

bool GLInit(){
    HINSTANCE hInst = GetModuleHandleW(nullptr);
    WNDCLASSW wc = {};
    wc.lpfnWndProc = DefWindowProcW;
    wc.hInstance = hInst;
    wc.lpszClassName = L"GLHidden";
    RegisterClassW(&wc);

    glc.hWnd = CreateWindowExW(0, L"GLHidden", L"",
        WS_POPUP, 0, 0, 4, 4, nullptr, nullptr, hInst, nullptr);
    if (!glc.hWnd) return false;

    glc.hDC = GetDC(glc.hWnd);

    PIXELFORMATDESCRIPTOR pfd = {};
    pfd.nSize = sizeof(pfd);
    pfd.nVersion = 1;
    pfd.dwFlags = PFD_DRAW_TO_WINDOW | PFD_SUPPORT_OPENGL | PFD_DOUBLEBUFFER;
    pfd.iPixelType = PFD_TYPE_RGBA;
    pfd.cColorBits = 32;
    pfd.cDepthBits = 24;
    pfd.cStencilBits = 8;
    pfd.iLayerType = PFD_MAIN_PLANE;

    int pf = ChoosePixelFormat(glc.hDC, &pfd);
    if (!pf){ ReleaseDC(glc.hWnd, glc.hDC); DestroyWindow(glc.hWnd); glc.hDC = nullptr; glc.hWnd = nullptr; return false; }
    SetPixelFormat(glc.hDC, pf, &pfd);

    glc.hRC = wglCreateContext(glc.hDC);
    if (!glc.hRC){ ReleaseDC(glc.hWnd, glc.hDC); DestroyWindow(glc.hWnd); glc.hDC = nullptr; glc.hWnd = nullptr; return false; }

    wglMakeCurrent(glc.hDC, glc.hRC);

    if (!LoadGLExtensions()){
        wglMakeCurrent(nullptr, nullptr);
        wglDeleteContext(glc.hRC);
        ReleaseDC(glc.hWnd, glc.hDC);
        DestroyWindow(glc.hWnd);
        glc.hRC = nullptr; glc.hDC = nullptr; glc.hWnd = nullptr;
        return false;
    }

    glEnable(GL_BLEND);
    glBlendFunc(GL_SRC_ALPHA, GL_ONE_MINUS_SRC_ALPHA);
    glEnable(GL_LINE_SMOOTH);
    glHint(GL_LINE_SMOOTH_HINT, GL_NICEST);
    glEnable(GL_POINT_SMOOTH);
    glHint(GL_POINT_SMOOTH_HINT, GL_NICEST);

    wglMakeCurrent(nullptr, nullptr);
    return true;
}

void GLResize(int w, int h){
    if (w <= 0 || h <= 0) return;
    if (w == glc.fboW && h == glc.fboH) return;

    wglMakeCurrent(glc.hDC, glc.hRC);

    if (glc.fboTex) { glDeleteTextures(1, &glc.fboTex); glc.fboTex = 0; }
    if (glc.fbo)    { pglDeleteFramebuffers(1, &glc.fbo); glc.fbo = 0; }

    // Create FBO color texture
    glGenTextures(1, &glc.fboTex);
    glBindTexture(GL_TEXTURE_2D, glc.fboTex);
    glTexImage2D(GL_TEXTURE_2D, 0, GL_RGBA8, w, h, 0, GL_BGRA, GL_UNSIGNED_BYTE, nullptr);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE);

    // Create FBO
    pglGenFramebuffers(1, &glc.fbo);
    pglBindFramebuffer(GL_FRAMEBUFFER, glc.fbo);
    pglFramebufferTexture2D(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, glc.fboTex, 0);

    GLenum status = pglCheckFramebufferStatus(GL_FRAMEBUFFER);
    if (status != GL_FRAMEBUFFER_COMPLETE){
        pglDeleteFramebuffers(1, &glc.fbo); glc.fbo = 0;
        glDeleteTextures(1, &glc.fboTex); glc.fboTex = 0;
    }

    glc.fboW = w;
    glc.fboH = h;
    glc.readBuf.resize((size_t)w * h * 4);

    wglMakeCurrent(nullptr, nullptr);
}

void GLShutdown(){
    wglMakeCurrent(glc.hDC, glc.hRC);

    if (glc.fboTex)  { glDeleteTextures(1, &glc.fboTex); glc.fboTex = 0; }
    if (glc.fbo)     { pglDeleteFramebuffers(1, &glc.fbo); glc.fbo = 0; }

    wglMakeCurrent(nullptr, nullptr);

    if (glc.hRC) { wglDeleteContext(glc.hRC); glc.hRC = nullptr; }
    if (glc.hDC) { ReleaseDC(glc.hWnd, glc.hDC); glc.hDC = nullptr; }
    if (glc.hWnd){ DestroyWindow(glc.hWnd); glc.hWnd = nullptr; }
}

// ====== Frame ======

void GLBeginFrame(int w, int h){
    if (w <= 0 || h <= 0) return;
    if (w != glc.fboW || h != glc.fboH) GLResize(w, h);

    wglMakeCurrent(glc.hDC, glc.hRC);

    if (glc.fbo){
        pglBindFramebuffer(GL_FRAMEBUFFER, glc.fbo);
    }

    glViewport(0, 0, w, h);
    glMatrixMode(GL_PROJECTION);
    glLoadIdentity();
    glOrtho(0, (GLdouble)w, (GLdouble)h, 0, -1, 1);
    glMatrixMode(GL_MODELVIEW);
    glLoadIdentity();
}

void GLEndFrame(){
    glFinish(); // ensure all GL commands complete

    // Synchronous readback — simple and reliable
    if (glc.fbo){
        glReadPixels(0, 0, glc.fboW, glc.fboH, GL_BGRA, GL_UNSIGNED_BYTE, glc.readBuf.data());
        pglBindFramebuffer(GL_FRAMEBUFFER, 0);
    }

    wglMakeCurrent(nullptr, nullptr);
}

bool GLFrameValid(){
    return !glc.readBuf.empty() && glc.fboW > 0 && glc.fboH > 0;
}

// ====== Transforms ======

void GLPushTransform(float scaleX, float scaleY, float translateX, float translateY){
    glMatrixMode(GL_MODELVIEW);
    glPushMatrix();
    glTranslatef(translateX, translateY, 0);
    glScalef(scaleX, scaleY, 1);
}

void GLPopTransform(){
    glMatrixMode(GL_MODELVIEW);
    glPopMatrix();
}

// ====== Clip ======

void GLPushClip(int x, int y, int w, int h){
    glEnable(GL_SCISSOR_TEST);
    int scissorY = glc.fboH - (y + h);
    if (scissorY < 0) scissorY = 0;
    glScissor(x, scissorY, w, h);
}

void GLPopClip(){
    glDisable(GL_SCISSOR_TEST);
}

// ====== Color helper ======

static inline void SetGLColor(Gdiplus::Color c){
    glColor4f(c.GetR() / 255.f, c.GetG() / 255.f, c.GetB() / 255.f, c.GetA() / 255.f);
}

// ====== Primitives ======

void GLFillRect(float x, float y, float w, float h, Gdiplus::Color c){
    SetGLColor(c);
    glRectf(x, y, x + w, y + h);
}

void GLDrawRect(float x, float y, float w, float h, Gdiplus::Color c, float sw){
    SetGLColor(c);
    glLineWidth(sw);
    glBegin(GL_LINE_LOOP);
    glVertex2f(x, y);
    glVertex2f(x + w, y);
    glVertex2f(x + w, y + h);
    glVertex2f(x, y + h);
    glEnd();
}

void GLFillEllipse(float cx, float cy, float rx, float ry, Gdiplus::Color c){
    SetGLColor(c);
    int segments = std::max(12, (int)((rx + ry) * 1.5f));
    if (segments > 120) segments = 120;
    glBegin(GL_TRIANGLE_FAN);
    glVertex2f(cx, cy);
    for (int i = 0; i <= segments; ++i){
        float a = (float)i / (float)segments * 6.2831853f;
        glVertex2f(cx + cosf(a) * rx, cy + sinf(a) * ry);
    }
    glEnd();
}

void GLDrawEllipse(float cx, float cy, float rx, float ry, Gdiplus::Color c, float sw){
    SetGLColor(c);
    glLineWidth(sw);
    int segments = std::max(12, (int)((rx + ry) * 1.5f));
    if (segments > 120) segments = 120;
    glBegin(GL_LINE_LOOP);
    for (int i = 0; i < segments; ++i){
        float a = (float)i / (float)segments * 6.2831853f;
        glVertex2f(cx + cosf(a) * rx, cy + sinf(a) * ry);
    }
    glEnd();
}

void GLDrawLine(float x1, float y1, float x2, float y2, Gdiplus::Color c, float sw){
    SetGLColor(c);
    glLineWidth(sw);
    glBegin(GL_LINES);
    glVertex2f(x1, y1);
    glVertex2f(x2, y2);
    glEnd();
}

// ====== Stroke rendering ======

static void EmitThickSegment(PointF2 a, PointF2 b, float hw){
    float dx = b.x - a.x, dy = b.y - a.y;
    float len = sqrtf(dx*dx + dy*dy);
    if (len < 0.001f) return;
    float nx = -dy / len * hw, ny = dx / len * hw;
    glVertex2f(a.x + nx, a.y + ny);
    glVertex2f(a.x - nx, a.y - ny);
    glVertex2f(b.x + nx, b.y + ny);
    glVertex2f(b.x - nx, b.y - ny);
}

static void EmitRoundCap(PointF2 p, float hw, int segs){
    glBegin(GL_TRIANGLE_FAN);
    glVertex2f(p.x, p.y);
    for (int i = 0; i <= segs; ++i){
        float a = (float)i / (float)segs * 6.2831853f;
        glVertex2f(p.x + cosf(a) * hw, p.y + sinf(a) * hw);
    }
    glEnd();
}

void GLDrawStroke(const std::vector<PointF2>& pts, Gdiplus::Color c, float width, bool dashed){
    if (pts.size() < 2) return;
    SetGLColor(c);
    float hw = width * 0.5f;
    int capSegs = std::max(6, (int)(width * 1.5f));
    if (capSegs > 24) capSegs = 24;

    glBegin(GL_TRIANGLE_STRIP);
    EmitThickSegment(pts[0], pts[1], hw);
    glEnd();
    EmitRoundCap(pts[0], hw, capSegs);
    glBegin(GL_TRIANGLE_STRIP);

    for (size_t i = 2; i < pts.size(); ++i){
        glEnd();
        EmitRoundCap(pts[i - 1], hw, capSegs);
        glBegin(GL_TRIANGLE_STRIP);
        EmitThickSegment(pts[i - 1], pts[i], hw);
    }
    glEnd();
    EmitRoundCap(pts.back(), hw, capSegs);

    glLineWidth(std::max(1.0f, width * 0.3f));
    glBegin(GL_LINE_STRIP);
    for (auto& p : pts) glVertex2f(p.x, p.y);
    glEnd();
}

// ====== Shape strokes ======

void GLDrawShapeRect(float x, float y, float w, float h, Gdiplus::Color c, float sw, bool dashed){
    if (dashed){ GLDrawDashedRect(x, y, w, h, c, sw); return; }
    GLDrawRect(x, y, w, h, c, sw);
}

void GLDrawShapeEllipse(float x, float y, float w, float h, Gdiplus::Color c, float sw, bool dashed){
    float cx = x + w * 0.5f, cy = y + h * 0.5f, rx = w * 0.5f, ry = h * 0.5f;
    if (dashed){
        SetGLColor(c); glLineWidth(sw);
        glEnable(GL_LINE_STIPPLE); glLineStipple(1, 0x0F0F);
        int segs = std::max(12, (int)((rx + ry) * 1.5f));
        if (segs > 120) segs = 120;
        glBegin(GL_LINE_LOOP);
        for (int i = 0; i < segs; ++i){ float a = (float)i/(float)segs*6.2831853f; glVertex2f(cx+cosf(a)*rx,cy+sinf(a)*ry); }
        glEnd();
        glDisable(GL_LINE_STIPPLE);
    } else { GLDrawEllipse(cx, cy, rx, ry, c, sw); }
}

void GLDrawShapeTriangle(float x1, float y1, float x2, float y2, Gdiplus::Color c, float sw, bool dashed){
    float cx = (x1 + x2) * 0.5f;
    SetGLColor(c); glLineWidth(sw);
    if (dashed){ glEnable(GL_LINE_STIPPLE); glLineStipple(1, 0x0F0F); }
    glBegin(GL_LINE_LOOP);
    glVertex2f(cx, y1); glVertex2f(x1, y2); glVertex2f(x2, y2);
    glEnd();
    if (dashed) glDisable(GL_LINE_STIPPLE);
}

void GLDrawShapeLine(float x1, float y1, float x2, float y2, Gdiplus::Color c, float sw){
    GLDrawLine(x1, y1, x2, y2, c, sw);
}

void GLDrawShapeArrow(float x1, float y1, float x2, float y2, Gdiplus::Color c, float sw){
    SetGLColor(c); glLineWidth(sw);
    glBegin(GL_LINES);
    glVertex2f(x1, y1); glVertex2f(x2, y2);
    glEnd();
    float ang = atan2f(y2 - y1, x2 - x1);
    float len = 18.0f + sw * 1.5f, a1 = ang + 2.6f, a2 = ang - 2.6f;
    glBegin(GL_LINES);
    glVertex2f(x2, y2); glVertex2f(x2 + cosf(a1)*len, y2 + sinf(a1)*len);
    glVertex2f(x2, y2); glVertex2f(x2 + cosf(a2)*len, y2 + sinf(a2)*len);
    glEnd();
}

// ====== Dashed rect ======

void GLDrawDashedRect(float x, float y, float w, float h, Gdiplus::Color c, float sw){
    SetGLColor(c); glLineWidth(sw);
    glEnable(GL_LINE_STIPPLE); glLineStipple(1, 0x0F0F);
    glBegin(GL_LINE_LOOP);
    glVertex2f(x, y); glVertex2f(x + w, y);
    glVertex2f(x + w, y + h); glVertex2f(x, y + h);
    glEnd();
    glDisable(GL_LINE_STIPPLE);
}

void GLDrawRubberBand(float x, float y, float w, float h,
    Gdiplus::Color fill, Gdiplus::Color border, float sw){
    GLFillRect(x, y, w, h, fill);
    GLDrawRect(x, y, w, h, border, sw);
}

// ====== Texture ======

GLuint GLCreateTexture(int w, int h, const void* data, bool linear){
    GLuint tex = 0;
    glGenTextures(1, &tex);
    glBindTexture(GL_TEXTURE_2D, tex);
    glTexImage2D(GL_TEXTURE_2D, 0, GL_RGBA8, w, h, 0, GL_BGRA, GL_UNSIGNED_BYTE, data);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, linear ? GL_LINEAR : GL_NEAREST);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, linear ? GL_LINEAR : GL_NEAREST);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE);
    return tex;
}

void GLUpdateTexture(GLuint tex, int w, int h, const void* data){
    glBindTexture(GL_TEXTURE_2D, tex);
    glTexSubImage2D(GL_TEXTURE_2D, 0, 0, 0, w, h, GL_BGRA, GL_UNSIGNED_BYTE, data);
}

void GLDrawTexture(GLuint tex, float x, float y, float w, float h, bool bilinear){
    glBindTexture(GL_TEXTURE_2D, tex);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, bilinear ? GL_LINEAR : GL_NEAREST);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, bilinear ? GL_LINEAR : GL_NEAREST);
    glEnable(GL_TEXTURE_2D);
    glColor4f(1, 1, 1, 1);
    glBegin(GL_QUADS);
    glTexCoord2f(0, 0); glVertex2f(x, y);
    glTexCoord2f(1, 0); glVertex2f(x + w, y);
    glTexCoord2f(1, 1); glVertex2f(x + w, y + h);
    glTexCoord2f(0, 1); glVertex2f(x, y + h);
    glEnd();
    glDisable(GL_TEXTURE_2D);
}

void GLDeleteTexture(GLuint tex){
    if (tex) glDeleteTextures(1, &tex);
}
