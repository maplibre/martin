// @ts-nocheck

import React from "react";
import PropTypes from "prop-types";
import FieldString from "./FieldString";
import Block from "./Block";

function displayValue(value) {
  if (typeof value === "undefined" || value === null) return value;
  if (value instanceof Date) return value.toLocaleString();
  if (
    typeof value === "object" ||
    typeof value === "number" ||
    typeof value === "string"
  )
    return value.toString();
  return value;
}

function renderProperties(feature) {
  return Object.keys(feature.properties).map((propertyName) => {
    const property = feature.properties[propertyName];
    return (
      <Block key={propertyName} label={propertyName}>
        <FieldString
          value={displayValue(property)}
          style={{ backgroundColor: "transparent" }}
        />
      </Block>
    );
  });
}

function renderFeatureId(feature) {
  return (
    <Block key={"feature-id"} label={"feature_id"}>
      <FieldString
        value={displayValue(feature.id)}
        style={{ backgroundColor: "transparent" }}
      />
    </Block>
  );
}

function renderFeature(feature, idx) {
  return (
    <div key={`${feature.sourceLayer}-${idx}`}>
      <div className="maputnik-popup-layer-id">
        {feature.layer["source"]}: {feature.layer["source-layer"]}
        {feature.inspectModeCounter && (
          <span> × {feature.inspectModeCounter}</span>
        )}
      </div>
      <Block key={"property-type"} label={"$type"}>
        <FieldString
          value={feature.geometry.type}
          style={{ backgroundColor: "transparent" }}
        />
      </Block>
      {renderFeatureId(feature)}
      {renderProperties(feature)}
    </div>
  );
}

function removeDuplicatedFeatures(features) {
  let uniqueFeatures = [];

  features.forEach((feature) => {
    const featureIndex = uniqueFeatures.findIndex((feature2) => {
      return (
        feature.layer["source-layer"] === feature2.layer["source-layer"] &&
        JSON.stringify(feature.properties) ===
          JSON.stringify(feature2.properties)
      );
    });

    if (featureIndex === -1) {
      uniqueFeatures.push(feature);
    } else {
      if (uniqueFeatures[featureIndex].hasOwnProperty("inspectModeCounter")) {
        uniqueFeatures[featureIndex].inspectModeCounter++;
      } else {
        uniqueFeatures[featureIndex].inspectModeCounter = 2;
      }
    }
  });

  return uniqueFeatures;
}

class FeaturePropertyPopup extends React.Component {
  static propTypes = {
    features: PropTypes.array,
  };

  render() {
    const features = removeDuplicatedFeatures(this.props.features);
    return (
      <div className="maputnik-feature-property-popup">
        {features.map(renderFeature)}
      </div>
    );
  }
}

export default FeaturePropertyPopup;
