import React, { PureComponent } from 'react';

import 'normalize.css';
import GlobalStyle from './GlobalStyle';
import './index.css';
import octocat from './kotik.svg';
import urbica from './urbica-logo.svg';
import arrowurbica from './arrow_urbica.svg'

import Map from '../Map';
import Intro from '../Intro';
import Description from '../Description';

class App extends PureComponent {
  render() {
    return (
      <>
        <GlobalStyle />
        <Intro />
        <Description />
        <div className="martin_items">
         <div className="feature">
           <h1 className="feature-description">Turning Data into Vector Tiles</h1>
           <h2>Martin creates Mapbox Vector Tiles from any PostGIS table or view </h2>
         </div>
         <div className="feature">
           <h1 className="feature-description">Generating Tiles with Functions</h1>
           <h2>Martin is the only vector tile server capable of creating tiles using database functions directly</h2>
         </div>
         <div className="feature">
           <h1 className="feature-description">Filtering and Aggregating Data on the Fly</h1>
           <h2>Martin is ideal for large datasets as it allows passing parameters from a URL into a user function to filter features and aggregate attribute values</h2>
         </div>
        </div>
        <div className="try-it">This is a demo of how Martin works. We used 2017 New York City taxi trips dataset: about 114 million records and a 13GB database. Martin uses a database function to filter the data by selected dates, days of the week, and hours and to sum or average the numbers by areas.</div>
        <Map />
        <div className="development">
          <div>Start building with Martin!</div>
          <a href="https://github.com/urbica/martin" className="git-button">
            View on Github
            <img src={octocat} alt="octocat"/>
          </a>
        </div>
        <footer className="footer">
          <div className="footerdescription">MADE BY</div>
          <a href="https://urbica.co">
            <img src={urbica} alt="urbica" />
          </a>
          <a href="https://urbica.co" className="footerarrow">
            <img src={arrowurbica} alt="arrowurbica" />
          </a>
        </footer>
      </>
    );
  }
}

export default App;
