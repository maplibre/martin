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
    <Intro />
    <Description>Martin is an Open Source PostGIS vector tile server</Description>
    <Features />
    <TryIt>
      <p>
        This is a demo of how Martin works. We used 2017 New York City taxi trips dataset: about 114
        million records and a 13GB database.
      </p>
      <p>
        Martin uses a database function to filter the data by selected dates, days of the week, and
        hours and to sum or average the numbers by areas.
      </p>
    </TryIt>
    <MaplibreMap />
    <Development />
    <Footer />
  </ParallaxProvider>
);

export default App;
