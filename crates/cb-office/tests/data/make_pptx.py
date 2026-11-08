import sys, io, struct, zlib
from pptx import Presentation
from pptx.util import Inches, Pt, Emu
from pptx.dml.color import RGBColor
from pptx.enum.shapes import MSO_SHAPE, MSO_CONNECTOR
from pptx.enum.text import PP_ALIGN
from lxml import etree

def png(w, h, color):
    raw = b''.join(b'\x00' + bytes(color) * w for _ in range(h))
    def chunk(t, d): return struct.pack('>I', len(d)) + t + d + struct.pack('>I', zlib.crc32(t + d) & 0xffffffff)
    return b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 2, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(raw)) + chunk(b'IEND', b'')

prs = Presentation()
prs.slide_width, prs.slide_height = Inches(13.333), Inches(7.5)
# 1 title
s = prs.slides.add_slide(prs.slide_layouts[0])
s.shapes.title.text = '云板课堂演示'
s.placeholders[1].text = 'Cloudboard 27 渲染测试 Rendering Test'
# 2 bullets
s = prs.slides.add_slide(prs.slide_layouts[1])
s.shapes.title.text = '本节要点'
tf = s.placeholders[1].text_frame
tf.text = '勾股定理：直角三角形两直角边的平方和等于斜边的平方'
for lvl, t in [(1, 'a² + b² = c²'), (1, '已知两边可求第三边'), (0, '圆的周长 C = 2πr'), (0, 'English bullet with bold'), (1, 'Second level item')]:
    p = tf.add_paragraph(); p.text = t; p.level = lvl
tf.paragraphs[4].runs[0].font.bold = True
# 3 shapes
s = prs.slides.add_slide(prs.slide_layouts[5])
s.shapes.title.text = '图形'
r = s.shapes.add_shape(MSO_SHAPE.ROUNDED_RECTANGLE, Inches(0.8), Inches(1.8), Inches(3), Inches(1.6)); r.text = '圆角矩形'
e = s.shapes.add_shape(MSO_SHAPE.OVAL, Inches(4.3), Inches(1.8), Inches(2.4), Inches(1.6)); e.text = '椭圆'
e.fill.solid(); e.fill.fore_color.rgb = RGBColor(0xED, 0x7D, 0x31)
a = s.shapes.add_shape(MSO_SHAPE.RIGHT_ARROW, Inches(7.2), Inches(2.0), Inches(2.6), Inches(1.2))
st = s.shapes.add_shape(MSO_SHAPE.STAR_5_POINT, Inches(10.3), Inches(1.6), Inches(2.0), Inches(2.0))
st.fill.solid(); st.fill.fore_color.rgb = RGBColor(0xFF, 0xC0, 0x00)
t = s.shapes.add_shape(MSO_SHAPE.ISOSCELES_TRIANGLE, Inches(0.8), Inches(4.2), Inches(2.4), Inches(2.2))
d = s.shapes.add_shape(MSO_SHAPE.DIAMOND, Inches(3.8), Inches(4.2), Inches(2.4), Inches(2.2)); d.text = '判断'
c = s.shapes.add_connector(MSO_CONNECTOR.STRAIGHT, Inches(6.8), Inches(5.3), Inches(9.5), Inches(5.3))
c.line.width = Pt(3); c.line.color.rgb = RGBColor(0xC0, 0, 0)
ln = c.line._get_or_add_ln(); ln.append(etree.SubElement(ln, '{http://schemas.openxmlformats.org/drawingml/2006/main}tailEnd', type='triangle'))
tb = s.shapes.add_textbox(Inches(9.8), Inches(4.5), Inches(3), Inches(1.5))
tb.text_frame.text = '文本框：红色 18 号'
tb.text_frame.paragraphs[0].runs[0].font.color.rgb = RGBColor(0xE0, 0x20, 0x20)
tb.text_frame.paragraphs[0].runs[0].font.size = Pt(18)
# 4 picture + table
s = prs.slides.add_slide(prs.slide_layouts[5])
s.shapes.title.text = '图片与表格'
s.shapes.add_picture(io.BytesIO(png(320, 200, (40, 150, 220))), Inches(0.8), Inches(1.8), Inches(4), Inches(2.5))
rows, cols = 4, 3
tbl = s.shapes.add_table(rows, cols, Inches(5.5), Inches(1.8), Inches(7), Inches(3)).table
data = [['学科', '课时', '教师'], ['数学', '5', '王老师'], ['语文', '6', '李老师'], ['英语', '4', '张老师']]
for i in range(rows):
    for j in range(cols):
        tbl.cell(i, j).text = data[i][j]
# 5 animations
s = prs.slides.add_slide(prs.slide_layouts[5])
s.shapes.title.text = '动画'
b1 = s.shapes.add_shape(MSO_SHAPE.RECTANGLE, Inches(1), Inches(2), Inches(3), Inches(1.5)); b1.text = '第一步'
b2 = s.shapes.add_shape(MSO_SHAPE.RECTANGLE, Inches(5), Inches(2), Inches(3), Inches(1.5)); b2.text = '第二步（飞入）'
b3 = s.shapes.add_shape(MSO_SHAPE.RECTANGLE, Inches(9), Inches(2), Inches(3), Inches(1.5)); b3.text = '第三步'
def eff(id_, spid, preset, cls, sub=0, node='clickEffect'):
    return f'''<p:par><p:cTn id="{id_}" presetID="{preset}" presetClass="{cls}" presetSubtype="{sub}" fill="hold" nodeType="{node}"><p:stCondLst><p:cond delay="0"/></p:stCondLst><p:childTnLst><p:set><p:cBhvr><p:cTn id="{id_+100}" dur="1" fill="hold"><p:stCondLst><p:cond delay="0"/></p:stCondLst></p:cTn><p:tgtEl><p:spTgt spid="{spid}"/></p:tgtEl><p:attrNameLst><p:attrName>style.visibility</p:attrName></p:attrNameLst></p:cBhvr><p:to><p:strVal val="visible"/></p:to></p:set></p:childTnLst></p:cTn></p:par>'''
def click(id_, inner):
    return f'''<p:par><p:cTn id="{id_}" fill="hold"><p:stCondLst><p:cond delay="indefinite"/></p:stCondLst><p:childTnLst><p:par><p:cTn id="{id_+1}" fill="hold"><p:stCondLst><p:cond delay="0"/></p:stCondLst><p:childTnLst>{inner}</p:childTnLst></p:cTn></p:par></p:childTnLst></p:cTn></p:par>'''
ids = [sh.shape_id for sh in (b1, b2, b3)]
timing = f'''<p:timing xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:tnLst><p:par><p:cTn id="1" dur="indefinite" restart="never" nodeType="tmRoot"><p:childTnLst><p:seq concurrent="1" nextAc="seek"><p:cTn id="2" dur="indefinite" nodeType="mainSeq"><p:childTnLst>{click(3, eff(5, ids[0], 10, 'entr'))}{click(10, eff(12, ids[1], 2, 'entr', 8))}{click(20, eff(22, ids[2], 10, 'entr'))}</p:childTnLst></p:cTn><p:prevCondLst/><p:nextCondLst/></p:seq></p:childTnLst></p:cTn></p:par></p:tnLst></p:timing>'''
s._element.append(etree.fromstring(timing))
# 6 group
s = prs.slides.add_slide(prs.slide_layouts[6])
g = s.shapes.add_group_shape()
g.shapes.add_shape(MSO_SHAPE.OVAL, Inches(2), Inches(2), Inches(2), Inches(2)).text = 'A'
g.shapes.add_shape(MSO_SHAPE.OVAL, Inches(5), Inches(2), Inches(2), Inches(2)).text = 'B'
g.shapes.add_shape(MSO_SHAPE.RIGHT_ARROW, Inches(4.1), Inches(2.7), Inches(0.8), Inches(0.6))
prs.save(sys.argv[1])
print('ok')
