import React from 'react';

import Container from './Container';
import Title from './Title';
import Description from './Description';
import GitHubButton from '../GitHubButton';

const Intro = () => (
  <Container>
    <div>
      <Title>
        Martin
      </Title>
      <Description>
        Vector Tiles from Large Databases on the Fly
      </Description>
      <GitHubButton />
    </div>
  </Container>
);

export default Intro;
