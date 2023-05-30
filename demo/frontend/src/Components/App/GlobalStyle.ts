import { createGlobalStyle } from 'styled-components';
import 'normalize.css';

// vremenagroteskBook
import vremenagroteskbookWoff2 from './fonts/vremenagroteskbook.woff2';
import vremenagroteskbookWoff from './fonts/vremenagroteskbook.woff';

// vremenaGroteskBold
import vremenagroteskboldWoff2 from './fonts/vremenagroteskbold.woff2';
import vremenagroteskboldWoff from './fonts/vremenagroteskbold.woff';

export default createGlobalStyle`
  * {
    box-sizing: border-box;
  }

  @font-face {
    font-family: vremena;
    src: url(${vremenagroteskbookWoff2}) format('woff2'),
         url(${vremenagroteskbookWoff}) format('woff');
    font-weight: normal;
    font-style: normal;
  }

  @font-face {
    font-family: vremena;
    src: url(${vremenagroteskboldWoff2}) format('woff2'),
         url(${vremenagroteskboldWoff}) format('woff');
    font-weight: bold;
    font-style: normal;
  }

  body {
    font-family: vremena, sans-serif;
    -webkit-font-smoothing: antialiased;
    -moz-osx-font-smoothing: grayscale;
    
    color: white;

    background-image: linear-gradient(to bottom,  #0e0e1e 50%, #1c1c30);
  }
`;
