import sys, io, struct, zlib
from docx import Document
from docx.shared import Pt, Inches, RGBColor, Cm
from docx.enum.text import WD_ALIGN_PARAGRAPH
from docx.oxml.ns import qn
from docx.oxml import OxmlElement

def png(w, h, color):
    raw = b''.join(b'\x00' + bytes(color) * w for _ in range(h))
    def chunk(t, d): return struct.pack('>I', len(d)) + t + d + struct.pack('>I', zlib.crc32(t + d) & 0xffffffff)
    return b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 2, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(raw)) + chunk(b'IEND', b'')

d = Document()
st = d.styles['Normal']; st.font.name = 'Times New Roman'; st.font.size = Pt(12)
st.element.rPr.rFonts.set(qn('w:eastAsia'), '宋体')
d.add_heading('第一章 勾股定理', level=1)
p = d.add_paragraph('在直角三角形中，两条直角边的平方和等于斜边的平方。这一定理在中国古代被称为“商高定理”，在西方被称为毕达哥拉斯定理。它是几何学中最重要的定理之一。')
p.paragraph_format.first_line_indent = Pt(24)
p = d.add_paragraph()
p.add_run('加粗文字').bold = True
p.add_run('，')
p.add_run('斜体 italic').italic = True
p.add_run('，')
r = p.add_run('红色文字'); r.font.color.rgb = RGBColor(0xC0, 0, 0)
p.add_run('，以及')
r = p.add_run('下划线'); r.underline = True
p.add_run('。')
d.add_heading('要点', level=2)
for t in ['直角边 a 和 b', '斜边 c', '公式 a² + b² = c²']:
    d.add_paragraph(t, style='List Bullet')
for t in ['画出直角三角形', '测量三条边', '验证公式']:
    d.add_paragraph(t, style='List Number')
p = d.add_paragraph('居中的段落'); p.alignment = WD_ALIGN_PARAGRAPH.CENTER
t = d.add_table(rows=3, cols=3); t.style = 'Table Grid'
for i, row in enumerate([['姓名', '数学', '语文'], ['张三', '95', '88'], ['李四', '82', '91']]):
    for j, v in enumerate(row):
        t.cell(i, j).text = v
d.add_paragraph('下面是一张图片：')
d.add_picture(io.BytesIO(png(300, 150, (230, 120, 40))), width=Inches(3))
d.add_page_break()
d.add_heading('第二章 长文本', level=1)
for i in range(12):
    p = d.add_paragraph(f'第{i+1}段：' + '教育是国之大计、党之大计。培养什么人、怎样培养人、为谁培养人是教育的根本问题。' * 3)
    p.paragraph_format.first_line_indent = Pt(24)
sec = d.sections[0]
sec.header.paragraphs[0].text = 'Cloudboard 测试文档'
sec.header.paragraphs[0].alignment = WD_ALIGN_PARAGRAPH.CENTER
fp = sec.footer.paragraphs[0]; fp.alignment = WD_ALIGN_PARAGRAPH.CENTER
fp.add_run('第 ')
r = fp.add_run()
for kind, txt in [('begin', None), (None, 'PAGE'), ('separate', None), (None, None), ('end', None)]:
    if kind:
        e = OxmlElement('w:fldChar'); e.set(qn('w:fldCharType'), kind); r._r.append(e)
    elif txt:
        e = OxmlElement('w:instrText'); e.text = txt; r._r.append(e)
    else:
        e = OxmlElement('w:t'); e.text = '1'; r._r.append(e)
fp.add_run(' 页')
d.save(sys.argv[1]); print('ok')
