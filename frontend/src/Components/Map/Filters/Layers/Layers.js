import React, { PureComponent } from 'react';
import layers from '../../../../config/layers';

import Layer from './Layer';
import Title from './Title';
import Description from './Description';

class Layers extends PureComponent {
  toggleLayerHandler = layerId => () => {
    this.props.toggleLayer(layerId);
  };

  render() {
    const { visibleLayer } = this.props;

    return (
      layers.map((layer) => {
        const isLayerVisible = visibleLayer === layer.id;

        return (
          <Layer
            key={layer.id}
            onClick={this.toggleLayerHandler(layer.id)}
            isLayerVisible={isLayerVisible}
          >
            <Title>
              {layer.title}
            </Title>
            {isLayerVisible && (
              <Description>
                {layer.description}
              </Description>
            )}
          </Layer>
        );
      })
    );
  }
}

export default Layers;
