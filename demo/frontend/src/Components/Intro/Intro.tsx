import React from 'react'
import { Parallax } from 'react-scroll-parallax'

import Container from './Container'
import Title from './Title'
import Description from './Description'
import GitHubButton from '../GitHubButton'
import DocsButton from '../GitHubButton/DocsButton'

const Intro = () => (
  <Container>
    <Parallax translateY={[100, -50]}>
      <Title>Martin<br />Demo</Title>
      <Description>Vector Tiles from Large Databases on the Fly</Description>
      <GitHubButton />{' '}
      <DocsButton />
    </Parallax>
  </Container>
)

export default Intro
