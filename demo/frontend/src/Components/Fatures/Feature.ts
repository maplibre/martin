import styled from 'styled-components';

export default styled.div`
  position: relative;

  max-width: 710px;
  min-height: 544px;
  margin-right: 15vw;
  margin-bottom: 150px;

  @media (max-width: 500px) {
    height: initial;
    margin-right: initial;
  }

  &:before {
    content: '';

    position: absolute;
    top: 80px;
    left: 125px;

    width: 350px;
    height: 450px;

    border: 7px solid;

    border-image: linear-gradient(to bottom left, #7300FF, #351c83) 1;
    
    @media (max-width: 500px) {
      height: 380px;
    }
  }

  &:nth-child(2) {
    margin-right: calc(15vw - 500px);

    @media (max-width: 500px) {
      margin-right: initial;
    }
  }

  &:last-child {
  display: none;
    @media (max-width: 500px) {
       display: block;
       margin-bottom: 0;
    }
  }
`;
