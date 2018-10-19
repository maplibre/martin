import { createGlobalStyle } from 'styled-components';


import media from './media';
import fontSize from './fontSize';

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

  html, body {
    -webkit-overflow-scrolling: touch;
    font-size: 16px;
      ${media.more`font-size: ${fontSize.size.more}; line-height: ${fontSize.lineHeight.more};`}
      ${media.giant`font-size: ${fontSize.size.giant}; line-height: ${fontSize.lineHeight.giant};`}
      ${media.desktop`
        font-size: ${fontSize.size.desktop};
        line-height: ${fontSize.lineHeight.desktop};
      `}
      ${media.tablet`font-size: ${fontSize.size.tablet} line-height: ${fontSize.lineHeight.tablet};`}
      ${media.phone`font-size: ${fontSize.size.phone}; line-height: ${fontSize.lineHeight.phone};`}
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
