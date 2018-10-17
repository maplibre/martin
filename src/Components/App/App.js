import React, { PureComponent } from 'react';

import 'normalize.css';
import GlobalStyle from './GlobalStyle';
import './index.css';
import kotik from './kotik.svg';
import urbica from './urbica-logo.svg';

// import Map from '../Map';
import Intro from '../Intro';

class App extends PureComponent {
  render() {
    return (
      <>
        <GlobalStyle />
        <Intro />
        <div className="description">
          Martin is an Open Source PostGIS/MVT
          <br/>
          vector tiles server created by Urbica
        </div>
        <h2 className="feature">Turning Data into Vector Tiles</h2>
        <div className="feature-description">Martin creates Mapbox Vector Tiles from any PostGIS table or view</div>
        <h2 className="feature">Generating Tiles with Functions</h2>
        <div className="feature-description">
          Martin is the only vector tiles server capable of creating tiles using database functions directly
        </div>
        <h2 className="feature">Filtering and Aggregating Data on the Fly</h2>
        <div className="feature-description">
          Martin is ideal for large datasets as it allows passing parameters from a URL into a
          user function to filter the features and aggregate the attribute values
        </div>
        <div className="try-it">Try it yourself on NY TAXI TRIPS dataset</div>
        {/*<Map />*/}
        <div className="development">
          The project is still under development, so stay tuned.
          Being a free and open source project contributors and supporters are extremely welcome.
        </div>
        <a href="https://github.com/urbica/martin" className="get-started">
          GET STARTED
          <img src={kotik} alt="octocat"/>
        </a>
        <footer className="footer">
          made by
          <img src={urbica} alt="urbica"/>
        </footer>
      </>
    );
  }
}

export default App;
