import React from 'react'
import { Parallax } from 'react-scroll-parallax'

import Container from './Container'
import Title from './Title'
import Description from './Description'
import GitHubButton from '../GitHubButton'

const Intro = () => (
  <Container>
    <Parallax translateY={[100, -50]}>
      <Title>Martin</Title>
      <Description>Vector Tiles from Large Databases on the Fly</Description>
      <GitHubButton />
    </Parallax>
  </Container>
)

export default Intro
