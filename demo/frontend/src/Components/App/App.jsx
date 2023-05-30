import React from 'react';
import { ParallaxProvider } from 'react-scroll-parallax';

import GlobalStyle from './GlobalStyle';

import Intro from '../Intro';
import Description from '../Description';
import Features from '../Fatures';
import TryIt from '../TryIt';
import Map from '../Map';
import Development from '../Development/Development';
import Footer from '../Footer/Footer';

const App = () => (
  <ParallaxProvider>
    <GlobalStyle />
    <Intro />
    <Description>
      Martin is an Open Source PostGIS vector tile server
    </Description>
    <Features />
    <TryIt>
      <p>
        This is a demo of how Martin works. We used 2017 New York City taxi
        trips dataset: about 114 million records and a 13GB database.
      </p>
      <p>
        Martin uses a database function to filter the data by selected dates,
        days of the week, and hours and to sum or average the numbers by areas.
      </p>
    </TryIt>
    <Map />
    <Development />
    <Footer />
  </ParallaxProvider>
);

export default App;
