export default (layer, painProperty) => {
  const fromColor = layer.paint[painProperty][4];
  const lastItemIndex = layer.paint[painProperty].length - 1;
  const toColor = layer.paint[painProperty][lastItemIndex];

  return [fromColor, toColor];
};
