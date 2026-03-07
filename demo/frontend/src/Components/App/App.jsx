import { ParallaxProvider } from "react-scroll-parallax";
import Description from "../Description";
import Development from "../Development/Development";
import Features from "../Fatures";
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
    <Description>Martin is an Open Source PostGIS vector tile server</Description>
    <Features />
    <TryIt>
      <p>
       This demo uses a 2017 New York City taxi trips dataset — 114 million records served as vector tiles.
      </p>
      <p>
       Martin uses a database function to filter data by date, day of week, and hour, and to aggregate values by area.
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
