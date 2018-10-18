import { createGlobalStyle } from 'styled-components'

// vremanaGroteskBook
import vremanaGroteskBook from './assets/fonts/VremenaGroteskBook.otf';
import vremanaGroteskBold from './assets/fonts/VremenaGroteskBold.otf';


export default createGlobalStyle`
  * {
    box-sizing: border-box;
  }

  @font-face {
    font-family: VremanaGrotesk;
    src:
      url(${vremanaGroteskBook}) format('otf'),
      
    font-weight: normal;
    font-style: normal;
  }

  @font-face {
    font-family: VremanaGrotesk;
    src:
      url(${vremanaGroteskBold}) format('otf'),
      
    font-weight: bold;
    font-style: normal;
  }

  
  body {
    background-image: linear-gradient(to bottom,  #0e0e1e 50%, #1c1c30);
    font-family: VremanaGrotesk, sans-serif;
    
    
  }
`;
