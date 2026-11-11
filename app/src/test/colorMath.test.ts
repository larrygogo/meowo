import { describe, expect, it } from "vitest";
import { contrast, deltaELab, over, parseColor } from "./colorMath";

describe("颜色算法自检", () => {
  it("CIEDE2000 对上 Sharma 2005 的标准样例", () => {
    expect(deltaELab([50, 2.6772, -79.7751], [50, 0, -82.7485])).toBeCloseTo(2.0425, 3);
    expect(deltaELab([50, 2.5, 0], [73, 25, -18])).toBeCloseTo(27.1492, 3);
    expect(deltaELab([60.2574, -34.0099, 36.2677], [60.4626, -34.1751, 39.4387])).toBeCloseTo(1.2644, 3);
  });
  it("对比度与合成", () => {
    expect(contrast(parseColor("#000"), parseColor("#fff"))).toBeCloseTo(21, 5);
    expect(over(parseColor("rgba(0, 0, 0, 0.5)"), parseColor("#ffffff"))[0]).toBeCloseTo(127.5, 5);
  });
});
