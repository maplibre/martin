import { ParallaxProvider } from "react-scroll-parallax";
import Description from "../Description";
import Development from "../Development/Development";
import Footer from "../Footer/Footer";
import Intro from "../Intro";
import MaplibreMap from "../MaplibreMap";
import TryIt from "../TryIt";
import GlobalStyle from "./GlobalStyle";

const App = () => (
  <ParallaxProvider>
    <GlobalStyle />
    <Head />
    <Intro />
    <Description>Martin is an open source tile server for PostGIS, PMTiles, MBTiles and Cloud Optimized GeoTIFFs</Description>
    <TryIt>
      <p>
        This demo serves 114 million 2017 New York City taxi trips as vector tiles, filtered and aggregated on the fly by a database function.
      </p>
    </TryIt>
    <MaplibreMap />
    <Development />
    <Footer />
  </ParallaxProvider>
);

const Head = () => (
    <div className="header">
        <div className="header-left"><img src="logo.png"/></div>
        <div className="header-right"><img src="tiles.png"/></div>
    </div>
);

export default App;
