export function hexToRgbA(hex: string, alpha: number = 0.6) {
  let c;
  if (/^#([A-Fa-f0-9]{3}){1,2}$/.test(hex)) {
    c = hex.substring(1).split("");
    if (c.length === 3) {
      c = [c[0], c[0], c[1], c[1], c[2], c[2]];
    }
    c = "0x" + c.join("");
    return `rgba(${[
      (Number(c) >> 16) & 255,
      (Number(c) >> 8) & 255,
      Number(c) & 255,
    ].join(",")}
            ,${alpha})`;
  }
  throw new Error("Bad Hex");
}

export const hexToRgbaArray = (
  hex: string,
  alpha: number = 1,
): [number, number, number, number] => {
  const rgbaString = hexToRgbA(hex, alpha);
  const [r, g, b] = rgbaString
    .split("rgba(")[1]
    .split(")")[0]
    .replace(/\s/g, "")
    .split(",")
    .map(Number);

  return [r, g, b, Math.floor(alpha * 255)];
};

const valueToHex = (val: number) => val.toString(16);

export const rgbArrayToHex = (rgb: number[]) => {
  return `#${valueToHex(rgb[0])}${valueToHex(rgb[1])}${valueToHex(rgb[2])}`;
};

export const validateColorHex = (color: string) =>
  /^#[0-9A-F]{6}$/i.test(color);
