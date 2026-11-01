// 内置字体集的**唯一**入口(app 主入口与预览/演示页都 import 这里,不再各自散抄):
// Inter 可变做西文;Noto Sans SC 可变做中文,对齐 Kimi Code 桌面的字体栈。
// 中文字体曾为安装包体积移除(子集约 5MB),用户 2026-10-29 明确选择以体积换观感一致,
// 重新内置;按 unicode-range 切成百来个子集,运行时只加载用到的那几片。
// 只引 latin + latin-ext 两个子集:裸 import "@fontsource-variable/inter" 会把
// cyrillic/greek/vietnamese 共 ~85KB 一并打进安装包,而应用只有中/英两种界面语言,
// 那些子集是纯死重量(浏览器按 unicode-range 不会下载,但 woff2 文件全部进产物)。
// latin-ext 保留:西欧人名/地名的变音符号(é/ü/ñ)在 transcript 里常见。
// 该包不提供按子集拆分的 CSS,声明住在本地 inter-subsets.css。
import "./inter-subsets.css";
import "@fontsource-variable/noto-sans-sc";
