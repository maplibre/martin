import { css } from 'styled-components';
import sizes from './viewportSize';

export default Object.keys(sizes).reduce((accumulator, label) => {
  const emSize = sizes[label] / 16;
  const result = accumulator;
  result[label] = (...args) => css`
      @media (max-width: ${emSize}em) {
        ${css(...args)};
      }
    `;
  return result;
}, {});
