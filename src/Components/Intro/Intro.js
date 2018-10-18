import React from 'react';
import './Intro.css';

import octocat from './kotik.svg';

const Intro = () => (
  <div className="intro">
    <div className="introblock">
      
      <div className="martin">Martin</div>
      <div className="slogan">Vector Tiles from Large Databases on the Fly</div>
      <a href="https://github.com/urbica/martin" className="gitbutton">
          View on Github
          <img src={octocat} alt="octocat"/>
      </a>
    </div>
  </div>
);

export default Intro;
