import React from 'react';
import { Parallax } from 'react-scroll-parallax';

import martinFeatures from '../../config/features';

import Container from './Container';
import Feature from './Feature';
import Title from './Title';
import Description from './Description';

import tiles from './martin_mobile.png';

const Features = () => (
  <Container>
    {martinFeatures.map(feature => (
      <Feature key={feature.id}>
        <Parallax
          offsetYMax={50}
          offsetYMin={-50}
          offsetXMin={20}
          offsetXMax={0}
          // slowerScrollRate
        >
          <Title>
            {feature.title}
          </Title>
        </Parallax>
        <Parallax
          offsetYMax={50}
          offsetYMin={-40}
          offsetXMin={0}
          offsetXMax={10}
          // slowerScrollRate
        >
          <Description>
            {feature.description}
          </Description>
        </Parallax>
      </Feature>
    ))}
    <Feature>
      <Title>
        <img src={tiles} alt='tiles' />
      </Title>
      <Parallax
        offsetYMax={50}
        offsetYMin={-40}
        offsetXMin={0}
        offsetXMax={10}
        // slowerScrollRate
      >
        <Description>
          Start building with Martin!
        </Description>
      </Parallax>
    </Feature>
  </Container>
);

export default Features;
