// 内置字体集的**唯一**入口(app 主入口与预览/演示页都 import 这里,不再各自散抄):
// Inter 可变做西文(~130KB,自托管全平台一致);中文走系统字体(Win 微软雅黑 / macOS 苹方,
// 兜底见 styles.css 的 font-family 栈)——此前内置的 Noto Sans SC 子集约 5MB、woff2 几乎
// 不可再压,是安装包最大的单一成分,为跨平台字形一致不值这个价。
import "@fontsource-variable/inter";
