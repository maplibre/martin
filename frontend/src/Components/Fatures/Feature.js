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

    width: 400px;
    height: 450px;

    border: 7px solid #7300FF;
    
    @media (max-width: 500px) {
      height: 380px;
    }
  }

  &:nth-child(2) {
    align-self: flex-end;

    margin-right: initial;
  }
`;
