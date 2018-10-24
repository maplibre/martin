/* eslint-disable max-len */
import { createGlobalStyle } from 'styled-components';

// vremenaGroteskBook
import vremenaGroteskBook from './assets/fonts/VremenaGroteskBook.otf';
import vremenaGroteskBookWoff2 from './assets/fonts/VremenaGroteskBookWoff2.woff2';

// vremenaGroteskBold
import vremenaGroteskBold from './assets/fonts/VremenaGroteskBold.otf';
import vremenaGroteskBoldWoff2 from './assets/fonts/VremenaGroteskBoldWoff2.woff2';


export default createGlobalStyle`
  * {
    box-sizing: border-box;
  }

  @font-face {
    font-family: VremenaGroteskBook;
    src:
      url(${vremenaGroteskBook}) format('otf'),
      url(${vremenaGroteskBookWoff2}) format('woff2');
    font-weight: normal;
    font-style: normal;
  }

  @font-face {
    font-family: VremenaGroteskBold;
    src:
      url(${vremenaGroteskBold}) format('otf'),
      url(${vremenaGroteskBoldWoff2}) format('woff2');
    font-weight: bold;
    font-style: normal;
  }

  body {
    background-image: linear-gradient(to bottom,  #0e0e1e 50%, #1c1c30);
    font-family: VremenaGroteskBook, sans-serif;
    -webkit-font-smoothing: antialiased;
    -moz-osx-font-smoothing: grayscale;    
  }

  h1 {
    font-family: VremenaGroteskBold, sans-serif;
  }
`;
