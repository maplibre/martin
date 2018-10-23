import styled from 'styled-components';

export default styled.div`
  padding: 5px;
  border-radius: 5px;
  margin-bottom: 15px;
  
  cursor: pointer;

  ${({ isLayerVisible }) => (isLayerVisible ? `
    box-shadow: 0 1px 4px 0 rgba(0, 0, 0, 0.5);
    background-color: rgba(18,17,30,0.55);
  ` : null)}
`;
