import styled from 'styled-components';

export default styled.div`
  position: absolute;
  top: 0;

  width: 350px;
  padding: 20px;
  border-bottom-right-radius: 5px;
  background-color: rgba(42, 42, 68, 0.9);
  color: #dadfee;

  z-index: 3;
  height: 615px;
  
  
  @media (max-width: 500px) {
    position: initial;

    width: 100%;
  }
`;
